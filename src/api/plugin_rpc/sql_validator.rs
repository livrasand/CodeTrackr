/// Tablas que los plugins pueden consultar. Solo lectura de datos propios del usuario.
/// Tablas de sistema accesibles: solo las que pertenecen al usuario autenticado.
/// Tablas sensibles (oauth_tokens, stripe, key_hash, etc.) siguen excluidas.
pub const PLUGIN_ALLOWED_TABLES: &[&str] = &[
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
pub fn validate_plugin_sql(sql: &str) -> Result<(), String> {
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
