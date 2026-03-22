#![allow(dead_code)]
//! SQL validation layer for the plugin sandbox.
//!
//! Plugins can execute SQL against the database, but every query must pass through
//! this validator before reaching sqlx. The validator operates on a real lexer
//! (token-based), not on raw string matching, so it is immune to bypass techniques
//! based on whitespace, unicode normalization, comment injection, or string literal
//! smuggling.
//!
//! # Security model
//! 1. Only `SELECT`, `INSERT`, `UPDATE`, `DELETE` are permitted as the first token.
//! 2. Set operations (`UNION`, `INTERSECT`, `EXCEPT`) and CTEs (`WITH`) are blocked.
//! 3. Table access is restricted to [`PLUGIN_ALLOWED_TABLES`].
//! 4. Queries on user-scoped tables must include a `user_id = $N` token filter.
//! 5. Sensitive columns on the `users` table (email, OAuth IDs, billing fields) are
//!    blocked at the token level — `SELECT *` on `users` is also rejected.
//! 6. System catalog tables (`information_schema`, `pg_catalog`, etc.) are blocked.

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

// Importar la función compartida de db.rs
use crate::db::find_dollar_quote_end;

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
                // Usar la función compartida para dollar-quotes
                let sql_str: String = chars.iter().collect();
                if let Some(end_pos) = find_dollar_quote_end(&sql_str, i) {
                    i = end_pos;
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
                    "NOW", "CURRENT_TIMESTAMP", "CAST", "ANY",
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

/// Validates SQL from a plugin script against a token-based allowlist.
///
/// # Security model
/// Uses a real lexer instead of string matching to prevent bypass via whitespace
/// tricks, unicode escapes, comment injection, or string literal smuggling.
/// The first token must be an allowed DML command; set operations and CTEs are
/// always rejected; table names are checked against [`PLUGIN_ALLOWED_TABLES`];
/// and user-scoped tables require an explicit `user_id = $N` guard.
///
/// # Errors
/// Returns `Err` with a human-readable message if the SQL:
/// - is empty
/// - starts with a disallowed command (e.g. `DROP`, `CREATE`, `TRUNCATE`)
/// - references a table outside the allowlist
/// - queries a user-scoped table without a `user_id = $N` filter
/// - accesses blocked columns on the `users` table
/// - uses `SELECT *` on the `users` table
/// - contains `UNION`, `INTERSECT`, `EXCEPT`, `WITH`, or unbalanced parentheses
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

    // 2. Rechazar keywords estructuralmente peligrosos y validar paréntesis contextualmente
    let mut paren_depth = 0;
    for (i, tok) in tokens.iter().enumerate() {
        match tok {
            SqlToken::Keyword(kw) => match kw.as_str() {
                "UNION" | "INTERSECT" | "EXCEPT" =>
                    return Err(format!("'{}' is not allowed in plugin queries", kw)),
                "WITH" =>
                    return Err("CTEs (WITH) are not allowed in plugin queries".to_string()),
                "AS" => {
                    // Permitir AS solo en contextos seguros: CAST y RETURNING
                    // Rechazar AS para aliases de tablas/columnas (que pueden ofuscar)
                    let is_safe_context = {
                        // Verificar si estamos en un contexto CAST (buscando CAST hacia atrás)
                        let in_cast_context = {
                            let mut found_cast = false;
                            let mut j = i.saturating_sub(1);
                            while j > 0 {
                                match &tokens[j] {
                                    SqlToken::Keyword(kw) if kw.as_str() == "CAST" => {
                                        found_cast = true;
                                        break;
                                    }
                                    SqlToken::Keyword(_) => break, // Si encontramos otro keyword, no es CAST context
                                    _ => j -= 1,
                                }
                            }
                            found_cast
                        };
                        
                        // Verificar si estamos en RETURNING (INSERT ... RETURNING col AS new_col)
                        let in_returning = tokens.iter().take(i).any(|t| matches!(t, SqlToken::Keyword(kw) if kw.as_str() == "RETURNING"));
                        
                        in_cast_context || in_returning
                    };
                    
                    if !is_safe_context {
                        return Err("Table/column aliases (AS) are not allowed in plugin queries".to_string());
                    }
                },
                "INFORMATION_SCHEMA" | "PG_CATALOG" | "PG_CLASS" =>
                    return Err("System catalog access is not allowed in plugin queries".to_string()),
                _ => {}
            },
            SqlToken::Punct('(') => {
                // Permitir paréntesis solo para funciones legítimas o casos específicos
                let is_valid_paren = {
                    // Buscar si el token anterior es una función permitida
                    let prev_is_function = if i > 0 {
                        match &tokens[i-1] {
                            SqlToken::Keyword(kw) => matches!(kw.as_str(),
                                "COALESCE" | "COUNT" | "SUM" | "AVG" | "MAX" | "MIN" |
                                "NOW" | "CURRENT_TIMESTAMP" | "CAST" | "EXISTS" | "ANY" | "VALUES"
                            ),
                            _ => false,
                        }
                    } else { false };
                    
                    // Permitir ANY() para casos como WHERE id = ANY($1)
                    // Buscar patrón: Ident Punct('=') Punct('(') Keyword('ANY')
                    let is_any_pattern = if i >= 2 {
                        matches!(&tokens[i-2], SqlToken::Ident(_)) &&
                        matches!(&tokens[i-1], SqlToken::Punct('=')) &&
                        i + 1 < tokens.len() &&
                        matches!(&tokens[i+1], SqlToken::Keyword(kw) if kw.as_str() == "ANY")
                    } else { false };
                    
                    // Permitir paréntesis después de VALUES en INSERT
                    let is_values_pattern = if i > 0 {
                        matches!(&tokens[i-1], SqlToken::Keyword(kw) if kw.as_str() == "VALUES")
                    } else { false };
                    
                    // Permitir paréntesis para listas de columnas en INSERT/UPDATE
                    // Buscar patrón: Keyword("INSERT") ... Ident(table_name) Punct('(')
                    let is_column_list_pattern = {
                        let mut found_insert = false;
                        let mut found_table = false;
                        for j in (0..i).rev() {
                            match &tokens[j] {
                                SqlToken::Keyword(kw) if kw.as_str() == "INSERT" => {
                                    found_insert = true;
                                    break;
                                }
                                SqlToken::Ident(_) => {
                                    found_table = true;
                                }
                                _ => {}
                            }
                        }
                        found_insert && found_table
                    };
                    
                    prev_is_function || is_any_pattern || is_values_pattern || is_column_list_pattern
                };
                
                if !is_valid_paren {
                    return Err("Parentheses are only allowed for functions (COALESCE, COUNT, SUM, ANY, etc.), VALUES clauses, and column lists".to_string());
                }
                paren_depth += 1;
            }
            SqlToken::Punct(')') => {
                if paren_depth == 0 {
                    return Err("Unbalanced parentheses".to_string());
                }
                paren_depth -= 1;
            }
            _ => {}
        }
    }
    
    if paren_depth != 0 {
        return Err("Unbalanced parentheses".to_string());
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
                    [SqlToken::Punct('='), SqlToken::Param]
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
