use std::sync::{Arc, Mutex};
use serde_json::{json, Value};

use crate::{AppState, realtime::RedisPool};
use super::sql_validator::validate_plugin_sql;

/// Fuerza el prefijo de namespace en claves Redis para aislar plugins entre sí.
fn plugin_redis_key(plugin_name: &str, user_id: &str, key: &str) -> String {
    format!("plugin:{}:{}:{}", plugin_name, user_id, key)
}

/// Ejecuta el handler del plugin dentro de un sandbox QuickJS embebido.
/// ctx.db.query ejecuta contra la DB real (sqlx) via tokio::runtime::Handle.
/// ctx.redis.* ejecuta contra Redis real via RedisPool.
pub fn run_rpc_in_quickjs(
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
                    let mut conn = match redis_ref.get_conn().await {
                        Ok(c) => c,
                        Err(_) => return "null".to_string(),
                    };
                    let val: Option<String> = redis::cmd("GET")
                        .arg(&namespaced)
                        .query_async(&mut conn)
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
                    if let Ok(mut conn) = redis_ref.get_conn().await {
                        let _: () = redis::cmd("SET")
                            .arg(&namespaced).arg(&value)
                            .query_async(&mut conn)
                            .await
                            .unwrap_or(());
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
                    if let Ok(mut conn) = redis_ref.get_conn().await {
                        let _: () = redis::cmd("DEL")
                            .arg(&namespaced)
                            .query_async(&mut conn)
                            .await
                            .unwrap_or(());
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
                    let mut conn = match redis_ref.get_conn().await {
                        Ok(c) => c,
                        Err(_) => return 0,
                    };
                    redis::cmd("INCR")
                        .arg(&namespaced)
                        .query_async::<_, i64>(&mut conn)
                        .await
                        .unwrap_or(0)
                })
            })?;
            ctx_ref.globals().set("__redis_incr", func)?
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

/// Ejecuta un hook de lifecycle dentro de un sandbox QuickJS embebido.
/// Similar a run_rpc_in_quickjs pero para hooks de lifecycle que no retornan valores.
/// Los errores se loggean pero no se propagan.
pub async fn run_lifecycle_hook_in_quickjs(
    plugin_script: &str,
    hook_name: &str,
    plugin_name: &str,
    user_id: &str,
    event_data_json: &str,
    state: AppState,
) {
    use rquickjs::{Context, Runtime};

    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::warn!("[lifecycle] JS runtime error for plugin {} hook {}: {}", plugin_name, hook_name, e);
            return;
        }
    };
    rt.set_memory_limit(16 * 1024 * 1024);
    rt.set_max_stack_size(512 * 1024);

    let ctx = match Context::full(&rt) {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::warn!("[lifecycle] JS context error for plugin {} hook {}: {}", plugin_name, hook_name, e);
            return;
        }
    };

    // Configurar funciones de DB y Redis similares a run_rpc_in_quickjs
    let db_pool = state.db.pool.clone();
    let redis_pool = state.redis.clone();
    let tokio_handle = tokio::runtime::Handle::current();
    let plugin_name_owned = plugin_name.to_string();
    let user_id_owned = user_id.to_string();

    let script = build_lifecycle_sandbox(plugin_script, hook_name, user_id, event_data_json);

    if let Err(e) = ctx.with(|ctx_ref| -> Result<(), rquickjs::Error> {
        // Funciones __db_query, __redis_get, etc. (igual que en run_rpc_in_quickjs)

        // __db_query
        {
            let pool = db_pool.clone();
            let handle = tokio_handle.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |sql: String, params_json: String| -> String {
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
                        tracing::warn!("Plugin lifecycle db.query error: {e}");
                        format!("{{\"__error\": \"{}\"}}", e.to_string().replace('"', "'"))
                    }
                }
            })?;
            ctx_ref.globals().set("__db_query", func)?;
        }

        // __redis_get
        {
            let redis_ref = redis_pool.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    let mut conn = match redis_ref.get_conn().await {
                        Ok(c) => c,
                        Err(_) => return "null".to_string(),
                    };
                    let val: Option<String> = redis::cmd("GET")
                        .arg(&namespaced)
                        .query_async(&mut conn)
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

        // __redis_set
        {
            let redis_ref = redis_pool.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String, value: String| {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    if let Ok(mut conn) = redis_ref.get_conn().await {
                        let _: () = redis::cmd("SET")
                            .arg(&namespaced).arg(&value)
                            .query_async(&mut conn)
                            .await
                            .unwrap_or(());
                    }
                });
            })?;
            ctx_ref.globals().set("__redis_set", func)?;
        }

        // __redis_del
        {
            let redis_ref = redis_pool.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    if let Ok(mut conn) = redis_ref.get_conn().await {
                        let _: () = redis::cmd("DEL")
                            .arg(&namespaced)
                            .query_async(&mut conn)
                            .await
                            .unwrap_or(());
                    }
                });
            })?;
            ctx_ref.globals().set("__redis_del", func)?;
        }

        // __redis_incr
        {
            let redis_ref = redis_pool.clone();
            let handle = tokio_handle.clone();
            let pname = plugin_name_owned.clone();
            let uid = user_id_owned.clone();
            let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> i64 {
                let redis_ref = redis_ref.clone();
                let namespaced = plugin_redis_key(&pname, &uid, &key);
                handle.block_on(async move {
                    let mut conn = match redis_ref.get_conn().await {
                        Ok(c) => c,
                        Err(_) => return 0,
                    };
                    redis::cmd("INCR")
                        .arg(&namespaced)
                        .query_async::<_, i64>(&mut conn)
                        .await
                        .unwrap_or(0)
                })
            })?;
            ctx_ref.globals().set("__redis_incr", func)?;
        }

        let _: rquickjs::Value = ctx_ref.eval(script.as_bytes().to_vec())?;
        Ok(())
    }) {
        tracing::warn!("[lifecycle] JS eval error for plugin {} hook {}: {}", plugin_name, hook_name, e);
        return;
    }

    // Procesar jobs pendientes
    loop {
        match rt.execute_pending_job() {
            Ok(true) => continue,
            Ok(false) => break,
            Err(_) => break,
        }
    }
}

/// Construye el script JS completo para ejecutar en QuickJS.
/// ctx.db/redis son wrappers sincrónicos que llaman a las funciones Rust nativas expuestas.
pub fn build_rpc_sandbox(plugin_script: &str, handler: &str, user_id: &str, req_json: &str) -> String {
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

/// Construye el script JS completo para ejecutar hooks de lifecycle en QuickJS.
/// Similar a build_rpc_sandbox pero ejecuta hooks de lifecycle que no retornan valores.
pub fn build_lifecycle_sandbox(plugin_script: &str, hook_name: &str, user_id: &str, event_data_json: &str) -> String {
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

const event = {event_data_json};

// ── Plugin script ─────────────────────────────────────────────────────────────
{plugin_script}

// ── Dispatch ──────────────────────────────────────────────────────────────────
(function() {{
  if (typeof lifecycle === 'undefined' || typeof lifecycle["{hook_name}"] !== 'function') {{
    // Hook no definido, ignorar silenciosamente
    return;
  }}
  var p = lifecycle["{hook_name}"](ctx, event);
  if (p && typeof p.then === 'function') {{
    p.then(function() {{
      // Hook completado exitosamente
    }}).catch(function(e) {{
      __push_log('[ERROR] Lifecycle hook {hook_name} failed: ' + (e && e.message ? e.message : String(e)));
    }});
  }}
}})();
"#,
        user_id = user_id,
        event_data_json = event_data_json,
        plugin_script = plugin_script,
        hook_name = hook_name,
    )
}
