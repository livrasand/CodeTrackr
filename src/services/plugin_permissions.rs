//! Sistema de Permisos Declarativos para Plugins
//!
//! Reemplaza el lexer SQL por un sistema de permisos validados en tiempo de instalación.
//! Los plugins declaran qué tablas y columnas necesitan, y el servidor valida en deploy.

use std::sync::Arc;
use serde_json::Value;
use uuid::Uuid;
use sqlx::PgPool;

/// Permiso declarativo de un plugin sobre una tabla específica
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TablePermission {
    pub table_name: String,
    pub access_type: AccessType,
    pub allowed_columns: Option<Vec<String>>, // None = todas las columnas no sensibles
    pub requires_user_filter: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AccessType {
    Read,
    Write,
    Admin, // Read + Write + puede omitir user_filter en algunas tablas
}

/// Permisos completos de un plugin (cacheados en memoria)
#[derive(Debug, Clone)]
pub struct PluginPermissions {
    #[allow(dead_code)]
    pub plugin_id: Uuid,
    pub tables: std::collections::HashMap<String, TablePermission>,
    #[allow(dead_code)]
    pub has_external_access: bool,
}

/// Parse simple de SQL para extraer tablas y columnas (sin validación de seguridad)
#[derive(Debug)]
struct ParsedQuery {
    command_type: QueryType,
    tables: Vec<String>,
    columns: Vec<String>,
    has_user_filter: bool,
}

#[derive(Debug)]
enum QueryType {
    Select,
    Insert,
    Update,
    Delete,
}

/// Cache de permisos por plugin
static PERMISSIONS_CACHE: std::sync::OnceLock<Arc<tokio::sync::RwLock<std::collections::HashMap<Uuid, PluginPermissions>>>> = 
    std::sync::OnceLock::new();

/// Inicializa el cache de permisos
fn init_permissions_cache() {
    let cache = Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));
    PERMISSIONS_CACHE.set(cache).expect("Permissions cache already initialized");
}

/// Obtiene el cache de permisos
fn get_permissions_cache() -> Option<&'static Arc<tokio::sync::RwLock<std::collections::HashMap<Uuid, PluginPermissions>>>> {
    PERMISSIONS_CACHE.get()
}

/// Carga permisos desde la base de datos
pub async fn load_plugin_permissions(
    pool: &PgPool,
    plugin_id: Uuid,
) -> Result<PluginPermissions, String> {
    #[derive(sqlx::FromRow)]
    struct PermRow {
        table_name: String,
        access_type: String,
        allowed_columns: Option<Vec<String>>,
        requires_user_filter: bool,
        has_external_access: bool,
    }

    let rows = sqlx::query_as::<_, PermRow>(
        r#"
        SELECT 
            pp.table_name,
            pp.access_type,
            pp.allowed_columns,
            pp.requires_user_filter,
            p.has_external_access
        FROM plugin_permissions pp
        JOIN plugin_store p ON pp.plugin_id = p.id
        WHERE pp.plugin_id = $1
        "#,
    )
    .bind(plugin_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to load plugin permissions: {}", e))?;

    let mut tables = std::collections::HashMap::new();
    let mut has_external_access = false;

    for row in rows {
        if !has_external_access {
            has_external_access = row.has_external_access;
        }

        let access_type = match row.access_type.as_str() {
            "read" => AccessType::Read,
            "write" => AccessType::Write,
            "admin" => AccessType::Admin,
            _ => return Err("Invalid access type".to_string()),
        };

        tables.insert(
            row.table_name.clone(),
            TablePermission {
                table_name: row.table_name,
                access_type,
                allowed_columns: row.allowed_columns,
                requires_user_filter: row.requires_user_filter,
            },
        );
    }

    Ok(PluginPermissions {
        plugin_id,
        tables,
        has_external_access,
    })
}

/// Parse simple SQL para extraer tablas y columnas (sin validación de seguridad)
fn parse_sql_simple(sql: &str) -> Result<ParsedQuery, String> {
    let sql_lower = sql.to_lowercase();
    
    // Extraer tipo de comando
    let command_type = if sql_lower.starts_with("select") {
        QueryType::Select
    } else if sql_lower.starts_with("insert") {
        QueryType::Insert
    } else if sql_lower.starts_with("update") {
        QueryType::Update
    } else if sql_lower.starts_with("delete") {
        QueryType::Delete
    } else {
        return Err("Only SELECT, INSERT, UPDATE, DELETE are allowed".to_string());
    };

    // Extraer tablas (búsqueda simple)
    let mut tables = Vec::new();
    let table_keywords = ["from", "join", "into", "update"];
    
    for keyword in &table_keywords {
        let pattern = format!(" {} ", keyword);
        if let Some(pos) = sql_lower.find(&pattern) {
            let after_keyword = &sql[pos + keyword.len() + 2..];
            let table_name = after_keyword
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c| c == ',' || c == '(' || c == ')');
            
            if !table_name.is_empty() && !tables.contains(&table_name.to_string()) {
                tables.push(table_name.to_string());
            }
        }
    }

    // Para SELECT, extraer columnas
    let columns = if matches!(command_type, QueryType::Select) {
        if let Some(select_start) = sql_lower.find("select") {
            let select_part = &sql[select_start + 6..];
            if let Some(from_pos) = select_part.find("from") {
                let columns_part = &select_part[..from_pos];
                columns_part
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .collect()
            } else {
                vec!["*".to_string()]
            }
        } else {
            vec!["*".to_string()]
        }
    } else {
        Vec::new()
    };

    Ok(ParsedQuery {
        command_type,
        tables,
        columns,
        has_user_filter: sql_lower.contains("user_id") && sql_lower.contains('='),
    })
}

/// Valida que el query cumpla con los permisos declarativos
fn validate_query_permissions(
    parsed: &ParsedQuery,
    permissions: &PluginPermissions,
    _user_id: Uuid,
) -> Result<(), String> {
    for table_name in &parsed.tables {
        let table_perm = permissions.tables.get(table_name)
            .ok_or_else(|| format!("Access to table '{}' not declared in plugin permissions", table_name))?;

        // Validar tipo de acceso
        match parsed.command_type {
            QueryType::Select => {
                if !matches!(table_perm.access_type, AccessType::Read | AccessType::Admin) {
                    return Err(format!("Read access to table '{}' not permitted", table_name));
                }
            }
            QueryType::Insert | QueryType::Update | QueryType::Delete => {
                if !matches!(table_perm.access_type, AccessType::Write | AccessType::Admin) {
                    return Err(format!("Write access to table '{}' not permitted", table_name));
                }
            }
        }

        // Validar filtro de usuario si es requerido
        if table_perm.requires_user_filter && !parsed.has_user_filter {
            return Err(format!(
                "Table '{}' requires user_id filter but none found in query", table_name
            ));
        }

        // Validar columnas para SELECT
        if matches!(parsed.command_type, QueryType::Select) {
            if let Some(allowed_cols) = &table_perm.allowed_columns {
                for col in &parsed.columns {
                    if col == "*" {
                        return Err(format!(
                            "SELECT * not allowed on table '{}' - specify columns explicitly", table_name
                        ));
                    }
                    if !allowed_cols.contains(col) && col != "user_id" {
                        return Err(format!(
                            "Column '{}' not allowed on table '{}' in plugin", col, table_name
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Ejecuta query con permisos declarativos (reemplaza validate_plugin_sql)
pub async fn execute_plugin_query_with_permissions(
    pool: &PgPool,
    plugin_id: Uuid,
    user_id: Uuid,
    sql: &str,
    params: &[Value],
) -> Result<Vec<Value>, String> {
    // Inicializar cache si es necesario
    if get_permissions_cache().is_none() {
        init_permissions_cache();
    }

    // 1. Obtener permisos del plugin (con cache)
    let plugin_perms = {
        let cache = get_permissions_cache().unwrap();
        let perms_cache = cache.read().await;
        if let Some(cached) = perms_cache.get(&plugin_id) {
            cached.clone()
        } else {
            drop(perms_cache);
            let loaded = load_plugin_permissions(pool, plugin_id).await?;
            let mut perms_cache = cache.write().await;
            perms_cache.insert(plugin_id, loaded.clone());
            loaded
        }
    };

    // 2. Parsear SQL simple
    let parsed = parse_sql_simple(sql)?;

    // 3. Validar contra permisos declarativos
    validate_query_permissions(&parsed, &plugin_perms, user_id)?;

    // 4. Ejecutar con sqlx directamente
    execute_query(pool, sql, params).await
}

/// Ejecuta el query con sqlx (sin validación adicional)
async fn execute_query(
    pool: &PgPool,
    sql: &str,
    params: &[Value],
) -> Result<Vec<Value>, String> {
    let mut query = sqlx::query(sql);
    
    for param in params {
        query = match param {
            Value::String(s) => {
                if let Ok(uuid) =s.parse::<uuid::Uuid>() {
                    query.bind(uuid)
                } else {
                    query.bind(s.clone())
                }
            }
            Value::Number(n) => {
                if let Some(i) = n.as_i64() { query.bind(i) }
                else { query.bind(n.as_f64().unwrap_or(0.0)) }
            }
            Value::Bool(b) => query.bind(*b),
            Value::Null => query.bind(Option::<String>::None),
            other => query.bind(other.to_string()),
        };
    }

    let rows = query.fetch_all(pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    let json_rows: Vec<Value> = rows.iter().map(|row| {
        use sqlx::Column;
        use sqlx::Row;
        let mut map = serde_json::Map::new();
        for col in row.columns() {
            let val: Value = row.try_get_raw(col.ordinal())
                .ok()
                .and_then(|_| {
                    if let Ok(v) = row.try_get::<String, _>(col.ordinal()) {
                        Some(Value::String(v))
                    } else if let Ok(v) = row.try_get::<i64, _>(col.ordinal()) {
                        Some(Value::Number(serde_json::Number::from(v)))
                    } else if let Ok(v) = row.try_get::<bool, _>(col.ordinal()) {
                        Some(Value::Bool(v))
                    } else if let Ok(v) = row.try_get::<f64, _>(col.ordinal()) {
                        Some(Value::Number(serde_json::Number::from_f64(v).unwrap_or(serde_json::Number::from(0))))
                    } else {
                        Some(Value::Null)
                    }
                })
                .unwrap_or(Value::Null);
            map.insert(col.name().to_string(), val);
        }
        Value::Object(map)
    }).collect();

    Ok(json_rows)
}

/// Limpia el cache de permisos para un plugin específico
#[allow(dead_code)]
pub async fn clear_plugin_permissions_cache(plugin_id: Uuid) {
    if let Some(cache) = get_permissions_cache() {
        let mut perms_cache = cache.write().await;
        perms_cache.remove(&plugin_id);
    }
}
