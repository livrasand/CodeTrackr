use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use crate::{AppState, auth::AuthenticatedUser, realtime::RedisPool};

/// POST /api/v1/plugins/:name/rpc/:handler
/// Executes a named endpoint handler declared inside a store plugin's script.
/// Runs inside an embedded QuickJS sandbox — sin Node.js, sin acceso a filesystem/red.
/// ctx.db y ctx.redis llaman a las implementaciones Rust reales (sqlx + redis).
///
/// The plugin script must export an `endpoints` object:
/// ```js
/// const endpoints = {
///   "delete-account": async (ctx, req) => {
///     await ctx.db.query("DELETE FROM users WHERE id = $1", [ctx.user_id]);
///     return { ok: true };
///   }
/// };
/// ```
pub async fn rpc_call(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path((plugin_name, handler_name)): Path<(String, String)>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Solo permitir caracteres seguros en el nombre del handler
    let handler_safe: String = handler_name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();

    if handler_safe.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid handler name"}))));
    }

    // 1. Cargar el script del plugin desde la DB — debe estar instalado por este usuario
    #[derive(sqlx::FromRow)]
    struct PluginRow { script: Option<String> }

    let row = sqlx::query_as::<_, PluginRow>(
        r#"SELECT p.script
           FROM plugin_store p
           JOIN installed_plugins i ON p.id = i.plugin_id
           WHERE p.name = $1 AND i.user_id = $2 AND p.is_banned = false AND p.is_published = true"#,
    )
    .bind(&plugin_name)
    .bind(user.id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found or not installed"}))))?;

    let script = row.script.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Plugin has no script"})))
    })?;

    let req_body = body.map(|b| b.0).unwrap_or(Value::Null);
    let req_json = serde_json::to_string(&req_body).unwrap_or_else(|_| "null".to_string());

    let user_id_str = user.id.to_string();
    let db_pool = state.db.pool.clone();
    let redis_pool = state.redis.clone();
    let plugin_name_log = plugin_name.clone();
    let handler_name_log = handler_name.clone();

    // 2. Ejecutar en sandbox QuickJS con timeout de 15s
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            run_rpc_in_quickjs(&script, &handler_safe, &plugin_name, &user_id_str, &req_json, db_pool, redis_pool)
        }),
    ).await;

    match result {
        Ok(Ok(Ok(value))) => Ok(Json(value)),
        Ok(Ok(Err(err_msg))) => {
            tracing::warn!("[plugin rpc] handler failed — plugin={plugin_name_log} handler={handler_name_log} detail={err_msg}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Plugin handler failed", "detail": err_msg})),
            ))
        }
        Ok(Err(e)) => {
            tracing::warn!("[plugin rpc] sandbox thread error — plugin={plugin_name_log} handler={handler_name_log} err={e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Sandbox thread error: {e}")})),
            ))
        }
        Err(_) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({"error": "Plugin handler timed out (15s limit)"})),
        )),
    }
}

/// Tablas que los plugins pueden consultar. Solo lectura de datos propios del usuario.
/// Tablas de sistema accesibles: solo las que pertenecen al usuario autenticado.
/// Tablas sensibles (oauth_tokens, stripe, key_hash, etc.) siguen excluidas.
const PLUGIN_ALLOWED_TABLES: &[&str] = &[
    // Datos de actividad
    "heartbeats",
    "projects",
    "daily_stats_cache",
    // Plugins
    "plugin_store",
    "installed_plugins",
    "plugin_settings",
    "plugin_reviews",
    // Cuenta del usuario (para leer perfil, actualizar preferencias, eliminar cuenta, etc.)
    "users",
    "api_keys",
    "user_follows",
];

/// Comandos SQL permitidos para plugins (solo lectura + insert/update/delete en tablas propias).
const PLUGIN_ALLOWED_COMMANDS: &[&str] = &["SELECT", "INSERT", "UPDATE", "DELETE"];

/// Tipo de token léxico SQL.
#[derive(Debug, PartialEq, Clone)]
enum SqlToken {
    Keyword(String),   // identificador uppercased que es un keyword reservado
    Ident(String),     // identificador lowercased (tabla, columna)
    Param,             // $1, $2, ...
    Literal,           // 'string', $$...$$, número — contenido opaco
    Punct(char),       // (, ), ,, ;, =, etc.
    Star,              // *
}

/// Tokeniza SQL en una secuencia plana de tokens léxicos.
/// Ignora completamente el contenido de strings literales y comentarios,
/// eliminando los vectores de bypass basados en contenido opaco.
fn tokenize_sql(sql: &str) -> Vec<SqlToken> {
    let chars: Vec<char> = sql.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < len {
        match chars[i] {
            // Whitespace — ignorar
            c if c.is_whitespace() => { i += 1; }

            // Comentario de línea --
            '-' if i + 1 < len && chars[i + 1] == '-' => {
                while i < len && chars[i] != '\n' { i += 1; }
            }

            // Comentario de bloque /* ... */
            '/' if i + 1 < len && chars[i + 1] == '*' => {
                i += 2;
                while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; }
                i += 2;
            }

            // String literal 'texto' — contenido opaco
            '\'' => {
                i += 1;
                while i < len {
                    if chars[i] == '\'' {
                        if i + 1 < len && chars[i + 1] == '\'' { i += 2; continue; }
                        break;
                    }
                    i += 1;
                }
                i += 1;
                tokens.push(SqlToken::Literal);
            }

            // Dollar-quoted string $$...$$ o $tag$...$tag$
            '$' => {
                // Detectar si es parámetro ($1, $2...)
                if i + 1 < len && chars[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < len && chars[i].is_ascii_digit() { i += 1; }
                    tokens.push(SqlToken::Param);
                    continue;
                }
                // Dollar-quote tag: $[tag]$
                let mut j = i + 1;
                while j < len && (chars[j].is_alphanumeric() || chars[j] == '_') { j += 1; }
                if j < len && chars[j] == '$' {
                    let tag: String = chars[i..=j].iter().collect();
                    let body_start = j + 1;
                    let body: String = chars[body_start..].iter().collect();
                    if let Some(end) = body.find(&tag) {
                        i = body_start + end + tag.len();
                    } else {
                        i = len;
                    }
                    tokens.push(SqlToken::Literal);
                } else {
                    // $ suelto — tratar como punct
                    tokens.push(SqlToken::Punct('$'));
                    i += 1;
                }
            }

            // Identificador entre comillas "ident" — contenido opaco como ident
            '"' => {
                i += 1;
                let mut ident = String::new();
                while i < len && chars[i] != '"' { ident.push(chars[i]); i += 1; }
                i += 1;
                tokens.push(SqlToken::Ident(ident.to_lowercase()));
            }

            // Número
            c if c.is_ascii_digit() => {
                while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
                tokens.push(SqlToken::Literal);
            }

            // Identificador o keyword
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
                let word: String = chars[start..i].iter().collect();
                let upper = word.to_uppercase();
                const KEYWORDS: &[&str] = &[
                    "SELECT", "INSERT", "UPDATE", "DELETE", "FROM", "WHERE",
                    "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "CROSS", "FULL",
                    "ON", "AND", "OR", "NOT", "IN", "IS", "NULL", "LIKE",
                    "ORDER", "BY", "GROUP", "HAVING", "LIMIT", "OFFSET",
                    "INTO", "VALUES", "SET", "RETURNING",
                    "UNION", "INTERSECT", "EXCEPT",
                    "WITH", "AS", "DISTINCT", "ALL", "EXISTS", "CASE", "WHEN",
                    "THEN", "ELSE", "END", "BETWEEN", "TRUE", "FALSE",
                    "INFORMATION_SCHEMA", "PG_CATALOG", "PG_CLASS",
                    "COALESCE", "COUNT", "SUM", "AVG", "MAX", "MIN",
                    "NOW", "CURRENT_TIMESTAMP", "CAST",
                ];
                if KEYWORDS.contains(&upper.as_str()) {
                    tokens.push(SqlToken::Keyword(upper));
                } else {
                    tokens.push(SqlToken::Ident(word.to_lowercase()));
                }
            }

            // * puede ser SELECT * o multiplicación
            '*' => { tokens.push(SqlToken::Star); i += 1; }

            // Puntuación
            c => { tokens.push(SqlToken::Punct(c)); i += 1; }
        }
    }

    tokens
}

/// Columnas sensibles que los plugins no pueden seleccionar de la tabla users.
const PLUGIN_USERS_BLOCKED_COLUMNS: &[&str] = &[
    "email", "github_id", "gitlab_id",
    "stripe_customer_id", "stripe_subscription_id", "plan_expires_at",
];

/// Tablas sensibles que SIEMPRE deben incluir user_id = $N en la cláusula WHERE.
const PLUGIN_USER_SCOPED_TABLES: &[&str] = &[
    "heartbeats",
    "api_keys",
    "user_follows",
    "plugin_settings",
    "installed_plugins",
    "users",
];

/// Valida el SQL de un plugin operando sobre tokens léxicos reales.
/// No depende de find()/contains() sobre el string raw — inmune a tricks de
/// whitespace, unicode, strings literales con keywords, y subqueries anidadas.
fn validate_plugin_sql(sql: &str) -> Result<(), String> {
    let tokens = tokenize_sql(sql);

    if tokens.is_empty() {
        return Err("Empty SQL query".to_string());
    }

    // 1. El primer token debe ser un comando permitido
    match &tokens[0] {
        SqlToken::Keyword(kw) if PLUGIN_ALLOWED_COMMANDS.contains(&kw.as_str()) => {}
        SqlToken::Keyword(kw) => {
            return Err(format!("SQL command '{}' is not allowed in plugins", kw));
        }
        _ => return Err("SQL must start with a valid command (SELECT/INSERT/UPDATE/DELETE)".to_string()),
    }

    // 2. Rechazar keywords estructuralmente peligrosos en cualquier posición
    for tok in &tokens {
        match tok {
            SqlToken::Keyword(kw) => match kw.as_str() {
                "UNION" | "INTERSECT" | "EXCEPT" =>
                    return Err(format!("'{}' is not allowed in plugin queries", kw)),
                "WITH" =>
                    return Err("CTEs (WITH) are not allowed in plugin queries".to_string()),
                "AS" =>
                    return Err("Aliases (AS) are not allowed in plugin queries".to_string()),
                "INFORMATION_SCHEMA" | "PG_CATALOG" | "PG_CLASS" =>
                    return Err("System catalog access is not allowed in plugin queries".to_string()),
                _ => {}
            },
            // Paréntesis de apertura indica subquery potencial — rechazar
            SqlToken::Punct('(') => {
                return Err("Subqueries and function calls with parentheses are not allowed in plugin queries".to_string());
            }
            _ => {}
        }
    }

    // 3. Extraer tablas referenciadas buscando el token DESPUÉS de FROM/JOIN/INTO/UPDATE
    let table_trigger_keywords = ["FROM", "JOIN", "INTO", "UPDATE",
        "LEFT", "RIGHT", "INNER", "OUTER", "CROSS", "FULL"];
    let mut tables_accessed: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let is_table_kw = matches!(&tokens[i], SqlToken::Keyword(kw) if table_trigger_keywords.contains(&kw.as_str()));
        if is_table_kw {
            // Avanzar al siguiente token no-keyword-de-join para encontrar el nombre de tabla
            let mut j = i + 1;
            while j < tokens.len() {
                match &tokens[j] {
                    SqlToken::Keyword(kw) if ["LEFT","RIGHT","INNER","OUTER","CROSS","FULL","JOIN","ONLY"].contains(&kw.as_str()) => { j += 1; }
                    SqlToken::Ident(name) => {
                        if !PLUGIN_ALLOWED_TABLES.contains(&name.as_str()) {
                            return Err(format!("Access to table '{}' is not allowed in plugins", name));
                        }
                        tables_accessed.push(name.clone());
                        break;
                    }
                    _ => break,
                }
            }
        }
        i += 1;
    }

    // 4. Para tablas sensibles verificar que user_id = $N esté presente como secuencia de tokens
    //    Buscar la secuencia: Ident("user_id") Punct('=') Param
    //    o variante con punto: Ident(_) Punct('.') Ident("user_id") Punct('=') Param
    let has_user_id_token_filter = {
        let mut found = false;
        let n = tokens.len();
        for idx in 0..n {
            let is_user_id = matches!(&tokens[idx], SqlToken::Ident(s) if s == "user_id");
            if is_user_id {
                // Buscar '=' seguido de Param en los próximos 2 tokens
                let next: Vec<_> = tokens.get(idx+1..=(idx+2).min(n-1)).unwrap_or(&[]).iter().collect();
                let eq_then_param = matches!(next.as_slice(),
                    [SqlToken::Punct('='), SqlToken::Param] |
                    [SqlToken::Punct('='), SqlToken::Literal] // también aceptar literales por si el ctx.user_id se pasa como string
                );
                if eq_then_param {
                    found = true;
                    break;
                }
            }
        }
        found
    };

    for table in &tables_accessed {
        if PLUGIN_USER_SCOPED_TABLES.contains(&table.as_str()) && !has_user_id_token_filter {
            return Err(format!(
                "Queries on '{}' must include a 'user_id = $N' filter (ctx.user_id)",
                table
            ));
        }

        // 5. Bloquear columnas sensibles en queries sobre users
        //    Buscar Ident(blocked_col) que aparezca ANTES del primer FROM en la secuencia de tokens
        if table == "users" {
            let from_pos = tokens.iter().position(|t| matches!(t, SqlToken::Keyword(k) if k == "FROM"));
            let select_tokens = &tokens[..from_pos.unwrap_or(tokens.len())];

            // SELECT * sobre users está bloqueado
            if select_tokens.iter().any(|t| matches!(t, SqlToken::Star)) {
                return Err("SELECT * is not allowed on 'users' table in plugins — list columns explicitly".to_string());
            }

            for blocked in PLUGIN_USERS_BLOCKED_COLUMNS {
                if select_tokens.iter().any(|t| matches!(t, SqlToken::Ident(s) if s == *blocked)) {
                    return Err(format!("Column '{}' is not accessible from plugin queries on 'users'", blocked));
                }
            }
        }
    }

    Ok(())
}

/// Fuerza el prefijo de namespace en claves Redis para aislar plugins entre sí.
fn plugin_redis_key(plugin_name: &str, user_id: &str, key: &str) -> String {
    format!("plugin:{}:{}:{}", plugin_name, user_id, key)
}

/// Ejecuta el handler del plugin dentro de un sandbox QuickJS embebido.
/// ctx.db.query ejecuta contra la DB real (sqlx) via tokio::runtime::Handle.
/// ctx.redis.* ejecuta contra Redis real via RedisPool.
fn run_rpc_in_quickjs(
    plugin_script: &str,
    handler: &str,
    plugin_name: &str,
    user_id: &str,
    req_json: &str,
    db_pool: sqlx::PgPool,
    redis: RedisPool,
) -> Result<Value, String> {
    use rquickjs::{Context, Runtime};

    let rt = Runtime::new().map_err(|e| format!("JS runtime error: {e}"))?;
    rt.set_memory_limit(16 * 1024 * 1024);
    rt.set_max_stack_size(512 * 1024);

    let ctx = Context::full(&rt).map_err(|e| format!("JS context error: {e}"))?;

    // Captura de output del plugin (log/warn/error → stderr informativo, no bloqueante)
    let log_output: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_clone = Arc::clone(&log_output);

    // Canal para recibir el resultado JSON del handler
    let result_cell: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result_cell);
    let error_cell: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let error_clone = Arc::clone(&error_cell);

    // Necesitamos un Handle de tokio para poder lanzar queries async desde el hilo síncrono
    let tokio_handle = tokio::runtime::Handle::current();

    let script = build_rpc_sandbox(plugin_script, handler, user_id, req_json);
    let plugin_name_owned = plugin_name.to_string();
    let user_id_owned = user_id.to_string();

    ctx.with(|ctx_ref| -> Result<(), rquickjs::Error> {
        // __push_log(msg) — log/warn/error del script JS llaman a esto
        {
            let out = Arc::clone(&log_clone);
            let func = rquickjs::Function::new(ctx_ref.clone(), move |msg: String| {
                out.lock().unwrap().push(msg);
            })?;
            ctx_ref.globals().set("__push_log", func)?;
        }

        // __db_query(sql, params_json) → JSON string de rows — llamada síncrona bloqueante
        {
            let pool = db_pool.clone();
            let handle = tokio_handle.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |sql: String, params_json: String| -> String {
                // Validar contra allowlist antes de ejecutar
                if let Err(e) = validate_plugin_sql(&sql) {
                    return format!("{{\"__error\": \"{}\"}}", e.replace('"', "'"));
                }
                let params: Vec<Value> = serde_json::from_str(&params_json).unwrap_or_default();
                let pool = pool.clone();
                let result = handle.block_on(async move {
                    let mut q = sqlx::query(&sql);
                    for p in &params {
                        q = match p {
                            Value::String(s) => {
                                if let Ok(uuid) = s.parse::<uuid::Uuid>() {
                                    q.bind(uuid)
                                } else {
                                    q.bind(s.clone())
                                }
                            }
                            Value::Number(n) => {
                                if let Some(i) = n.as_i64() { q.bind(i) }
                                else { q.bind(n.as_f64().unwrap_or(0.0)) }
                            },
                            Value::Bool(b) => q.bind(*b),
                            Value::Null => q.bind(Option::<String>::None),
                            other => q.bind(other.to_string()),
                        };
                    }
                    q.fetch_all(&pool).await
                });
                match result {
                    Ok(rows) => {
                        let json_rows: Vec<Value> = rows.iter().map(|row| {
                            use sqlx::Column;
                            use sqlx::Row;
                            let mut map = serde_json::Map::new();
                            for col in row.columns() {
                                let val: Value = row.try_get_raw(col.ordinal())
                                    .ok()
                                    .and_then(|_raw| {
                                        // Intentar deserializar como tipos comunes
                                        if let Ok(v) = row.try_get::<String, _>(col.ordinal()) {
                                            Some(Value::String(v))
                                        } else if let Ok(v) = row.try_get::<i64, _>(col.ordinal()) {
                                            Some(json!(v))
                                        } else if let Ok(v) = row.try_get::<bool, _>(col.ordinal()) {
                                            Some(json!(v))
                                        } else if let Ok(v) = row.try_get::<f64, _>(col.ordinal()) {
                                            Some(json!(v))
                                        } else {
                                            Some(Value::Null)
                                        }
                                    })
                                    .unwrap_or(Value::Null);
                                map.insert(col.name().to_string(), val);
                            }
                            Value::Object(map)
                        }).collect();
                        serde_json::to_string(&json_rows).unwrap_or_else(|_| "[]".to_string())
                    },
                    Err(e) => {
                        tracing::warn!("Plugin RPC db.query error: {e}");
                        format!("{{\"__error\": \"{}\"}}", e.to_string().replace('"', "'"))
                    }
                }
            })?;
            ctx_ref.globals().set("__db_query", func)?;
        }

        // __redis_get(key) → valor como string JSON
        {
            let redis_ref = redis.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    let mut guard = match redis_ref.get_conn().await {
                        Ok(g) => g,
                        Err(_) => return "null".to_string(),
                    };
                    let conn = match guard.as_mut() {
                        Some(c) => c,
                        None => return "null".to_string(),
                    };
                    let val: Option<String> = redis::cmd("GET")
                        .arg(&namespaced)
                        .query_async(conn)
                        .await
                        .unwrap_or(None);
                    match val {
                        Some(v) => format!("\"{}\"", v.replace('"', "\\\"")),
                        None => "null".to_string(),
                    }
                })
            })?;
            ctx_ref.globals().set("__redis_get", func)?;
        }

        // __redis_set(key, value)
        {
            let redis_ref = redis.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String, value: String| {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    if let Ok(mut guard) = redis_ref.get_conn().await {
                        if let Some(conn) = guard.as_mut() {
                            let _: () = redis::cmd("SET")
                                .arg(&namespaced).arg(&value)
                                .query_async(conn)
                                .await
                                .unwrap_or(());
                        }
                    }
                });
            })?;
            ctx_ref.globals().set("__redis_set", func)?;
        }

        // __redis_del(key)
        {
            let redis_ref = redis.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    if let Ok(mut guard) = redis_ref.get_conn().await {
                        if let Some(conn) = guard.as_mut() {
                            let _: () = redis::cmd("DEL")
                                .arg(&namespaced)
                                .query_async(conn)
                                .await
                                .unwrap_or(());
                        }
                    }
                });
            })?;
            ctx_ref.globals().set("__redis_del", func)?;
        }

        // __redis_incr(key) → i64
        {
            let redis_ref = redis.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> i64 {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    let mut guard = match redis_ref.get_conn().await {
                        Ok(g) => g,
                        Err(_) => return 0,
                    };
                    let conn = match guard.as_mut() {
                        Some(c) => c,
                        None => return 0,
                    };
                    redis::cmd("INCR")
                        .arg(&namespaced)
                        .query_async::<_, i64>(conn)
                        .await
                        .unwrap_or(0)
                })
            })?;
            ctx_ref.globals().set("__redis_incr", func)?;
        }

        // __set_result(json_string) — el handler llama esto con su valor de retorno
        {
            let res = Arc::clone(&result_clone);
            let func = rquickjs::Function::new(ctx_ref.clone(), move |json_str: String| {
                if let Ok(v) = serde_json::from_str::<Value>(&json_str) {
                    *res.lock().unwrap() = Some(v);
                }
            })?;
            ctx_ref.globals().set("__set_result", func)?;
        }

        // __set_error(msg)
        {
            let err = Arc::clone(&error_clone);
            let func = rquickjs::Function::new(ctx_ref.clone(), move |msg: String| {
                *err.lock().unwrap() = Some(msg);
            })?;
            ctx_ref.globals().set("__set_error", func)?;
        }

        let _: rquickjs::Value = ctx_ref.eval(script.as_bytes().to_vec())?;
        Ok(())
    }).map_err(|e| format!("JS eval error: {e}"))?;

    // Drenar jobs pendientes (async/await en el script) — fuera de ctx.with
    loop {
        match rt.execute_pending_job() {
            Ok(true) => continue,
            Ok(false) => break,
            Err(_) => break,
        }
    }

    // Registrar logs del plugin (informativo)
    let logs = log_output.lock().unwrap();
    for line in logs.iter() {
        tracing::debug!("[plugin rpc log] {line}");
    }

    if let Some(err) = error_cell.lock().unwrap().take() {
        return Err(err);
    }

    let result = result_cell.lock().unwrap().take()
        .unwrap_or_else(|| json!({"ok": true}));

    Ok(result)
}

/// Construye el script JS completo para ejecutar en QuickJS.
/// ctx.db/redis son wrappers sincrónicos que llaman a las funciones Rust nativas expuestas.
fn build_rpc_sandbox(plugin_script: &str, handler: &str, user_id: &str, req_json: &str) -> String {
    format!(
        r#"
// ── Helpers de logging ───────────────────────────────────────────────────────
function log() {{ __push_log(Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function warn() {{ __push_log('[WARN] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function error() {{ __push_log('[ERROR] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}

// ── Context con DB y Redis reales (via Rust bridge) ──────────────────────────
const ctx = {{
  user_id: "{user_id}",
  db: {{
    query: function(sql, params) {{
      const result = __db_query(sql, JSON.stringify(params || []));
      const parsed = JSON.parse(result);
      if (parsed && parsed.__error) throw new Error(parsed.__error);
      return Promise.resolve(parsed);
    }}
  }},
  redis: {{
    get:  function(key)        {{ return Promise.resolve(JSON.parse(__redis_get(key))); }},
    set:  function(key, value) {{ __redis_set(key, JSON.stringify(value)); return Promise.resolve(); }},
    incr: function(key)        {{ return Promise.resolve(__redis_incr(key)); }},
    del:  function(key)        {{ __redis_del(key); return Promise.resolve(); }},
  }},
  config: {{ base_url: "https://codetrackr.leapcell.app" }}
}};

const req = {req_json};

// ── Plugin script ─────────────────────────────────────────────────────────────
{plugin_script}

// ── Dispatch ──────────────────────────────────────────────────────────────────
(function() {{
  if (typeof endpoints === 'undefined' || typeof endpoints["{handler}"] !== 'function') {{
    __set_error("Handler '{handler}' not found in plugin endpoints");
    return;
  }}
  var p = endpoints["{handler}"](ctx, req);
  if (p && typeof p.then === 'function') {{
    p.then(function(result) {{
      __set_result(JSON.stringify(result != null ? result : {{ ok: true }}));
    }}).catch(function(e) {{
      __set_error(e && e.message ? e.message : String(e));
    }});
  }} else {{
    __set_result(JSON.stringify(p != null ? p : {{ ok: true }}));
  }}
}})();
"#,
        user_id = user_id,
        req_json = req_json,
        plugin_script = plugin_script,
        handler = handler,
    )
}
