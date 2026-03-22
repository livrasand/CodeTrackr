/// Plugin Worker System
///
/// Implementa workers dedicados para ejecución de plugins con comunicación síncrona real.
/// Cada worker corre en su propio thread con un runtime QuickJS aislado.
///
/// Arquitectura:
/// - PluginWorkerPool: gestiona pool de workers
/// - PluginWorker: thread individual con QuickJS runtime
/// - Comunicación vía channels síncronos (oneshot para request/response)
/// - Aislamiento de memoria y recursos por worker

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use tokio::sync::{mpsc, oneshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use sqlx::PgPool;

use crate::AppState;

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
    pub plugin_id: Uuid,
    pub tables: std::collections::HashMap<String, TablePermission>,
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
    user_id: Uuid,
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
                if let Ok(uuid) = s.parse::<uuid::Uuid>() {
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
pub async fn clear_plugin_permissions_cache(plugin_id: Uuid) {
    if let Some(cache) = get_permissions_cache() {
        let mut perms_cache = cache.write().await;
        perms_cache.remove(&plugin_id);
    }
}

// Función dummy para get_worker_pool para compatibilidad temporal
pub fn get_worker_pool() -> Option<()> {
    None
}

/// Mensajes enviados a los workers
#[derive(Debug, Clone)]
pub enum WorkerMessage {
    /// Ejecutar RPC call en plugin
    ExecuteRpc {
        id: String,
        plugin_script: String,
        handler: String,
        plugin_name: String,
        user_id: String,
        req_json: String,
        plugin_id: uuid::Uuid,
        response_tx: OneshotSender<Result<Value, String>>,
    },
    /// Ejecutar lifecycle hook
    ExecuteLifecycle {
        id: String,
        plugin_script: String,
        hook_name: String,
        plugin_name: String,
        user_id: String,
        event_data_json: String,
        plugin_id: uuid::Uuid,
    },
    /// Obtener estadísticas del worker
    GetStats {
        response_tx: OneshotSender<WorkerStats>,
    },
    /// Detener el worker
    Shutdown,
}

type OneshotSender<T> = oneshot::Sender<T>;

/// Estadísticas del worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStats {
    pub worker_id: String,
    pub executions_count: u64,
    pub memory_usage_mb: u64,
    pub uptime_seconds: u64,
    pub last_execution: Option<String>,
}

/// Worker individual con su propio thread y QuickJS runtime
pub struct PluginWorker {
    pub id: String,
    handle: JoinHandle<()>,
    tx: mpsc::UnboundedSender<WorkerMessage>,
}

impl PluginWorker {
    /// Crea un nuevo worker en su propio thread
    pub fn new(worker_id: String, state: AppState) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<WorkerMessage>();
        
        let handle = thread::Builder::new()
            .name(format!("plugin-worker-{}", worker_id))
            .spawn(move || {
                Self::worker_loop(worker_id, rx, state);
            })
            .expect("Failed to spawn plugin worker thread");

        Self { id: worker_id, handle, tx }
    }

    /// Loop principal del worker thread
    fn worker_loop(
        worker_id: String,
        mut rx: mpsc::UnboundedReceiver<WorkerMessage>,
        state: AppState,
    ) {
        tracing::info!("[worker-{}] Plugin worker iniciado", worker_id);
        
        let start_time = std::time::Instant::now();
        let mut executions_count = 0u64;
        
        // Runtime QuickJS reutilizado en este thread
        let rt = match rquickjs::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("[worker-{}] Error creando QuickJS runtime: {}", worker_id, e);
                return;
            }
        };
        
        rt.set_memory_limit(16 * 1024 * 1024); // 16MB
        rt.set_max_stack_size(512 * 1024); // 512KB

        while let Some(msg) = rx.blocking_recv() {
            match msg {
                WorkerMessage::ExecuteRpc {
                    id,
                    plugin_script,
                    handler,
                    plugin_name,
                    user_id,
                    req_json,
                    plugin_id,
                    response_tx,
                } => {
                    executions_count += 1;
                    let _ = response_tx.send(Self::execute_rpc(
                        &rt,
                        &plugin_script,
                        &handler,
                        &plugin_name,
                        &user_id,
                        &req_json,
                        state.clone(),
                        plugin_id,
                    ));
                }
                
                WorkerMessage::ExecuteLifecycle {
                    id: _,
                    plugin_script,
                    hook_name,
                    plugin_name,
                    user_id,
                    event_data_json,
                    plugin_id,
                } => {
                    executions_count += 1;
                    Self::execute_lifecycle(
                        &rt,
                        &plugin_script,
                        &hook_name,
                        &plugin_name,
                        &user_id,
                        &event_data_json,
                        state.clone(),
                        plugin_id,
                    );
                }
                
                WorkerMessage::GetStats { response_tx } => {
                    let uptime = start_time.elapsed().as_secs();
                    let stats = WorkerStats {
                        worker_id: worker_id.clone(),
                        executions_count,
                        memory_usage_mb: 16, // Fixed limit
                        uptime_seconds: uptime,
                        last_execution: Some(chrono::Utc::now().to_rfc3339()),
                    };
                    let _ = response_tx.send(stats);
                }
                
                WorkerMessage::Shutdown => {
                    tracing::info!("[worker-{}] Apagando worker", worker_id);
                    break;
                }
            }
        }
        
        tracing::info!("[worker-{}] Worker finalizado", worker_id);
    }

    /// Ejecuta RPC call dentro del QuickJS runtime del worker
    fn execute_rpc(
        rt: &rquickjs::Runtime,
        plugin_script: &str,
        handler: &str,
        plugin_name: &str,
        user_id: &str,
        req_json: &str,
        state: AppState,
        plugin_id: uuid::Uuid,
    ) -> Result<Value, String> {
        use crate::api::plugin_rpc::sandbox::run_rpc_in_quickjs;
        
        run_rpc_in_quickjs(
            plugin_script,
            handler,
            plugin_name,
            user_id,
            req_json,
            state.db.pool.clone(),
            state.redis.clone(),
            plugin_id,
        )
    }

    /// Ejecuta lifecycle hook dentro del QuickJS runtime del worker
    fn execute_lifecycle(
        rt: &rquickjs::Runtime,
        plugin_script: &str,
        hook_name: &str,
        plugin_name: &str,
        user_id: &str,
        event_data_json: &str,
        state: AppState,
        plugin_id: uuid::Uuid,
    ) {
        use crate::api::plugin_rpc::sandbox::run_lifecycle_hook_in_quickjs;
        
        run_lifecycle_hook_in_quickjs(
            plugin_script,
            hook_name,
            plugin_name,
            user_id,
            event_data_json,
            state,
            plugin_id,
        );
    }

    /// Envía mensaje al worker (non-blocking)
    pub fn send(&self, msg: WorkerMessage) -> Result<(), mpsc::error::SendError<WorkerMessage>> {
        self.tx.send(msg)
    }

    /// Detiene el worker y espera a que termine
    pub fn shutdown(self) -> thread::Result<()> {
        let _ = self.send(WorkerMessage::Shutdown);
        self.handle.join()
    }
}

/// Pool de workers para balancear carga
pub struct PluginWorkerPool {
    workers: Vec<PluginWorker>,
    next_worker: Arc<Mutex<usize>>,
}

impl PluginWorkerPool {
    /// Crea un nuevo pool con el número especificado de workers
    pub fn new(size: usize, state: AppState) -> Self {
        let mut workers = Vec::with_capacity(size);
        
        for i in 0..size {
            let worker = PluginWorker::new(
                Uuid::new_v4().to_string(),
                state.clone(),
            );
            workers.push(worker);
        }
        
        tracing::info!("Plugin worker pool creado con {} workers", size);
        
        Self {
            workers,
            next_worker: Arc::new(Mutex::new(0)),
        }
    }

    /// Selecciona el siguiente worker usando round-robin
    fn get_next_worker(&self) -> &PluginWorker {
        let mut next = self.next_worker.lock().unwrap();
        let worker = &self.workers[*next];
        *next = (*next + 1) % self.workers.len();
        worker
    }

    /// Ejecuta RPC call en un worker del pool
    pub async fn execute_rpc(
        &self,
        plugin_script: &str,
        handler: &str,
        plugin_name: &str,
        user_id: &str,
        req_json: &str,
        plugin_id: uuid::Uuid,
        state: AppState,
    ) -> Result<Value, String> {
        let worker = self.get_next_worker();
        let (tx, rx) = oneshot::channel();
        
        let msg = WorkerMessage::ExecuteRpc {
            id: Uuid::new_v4().to_string(),
            plugin_script: plugin_script.to_string(),
            handler: handler.to_string(),
            plugin_name: plugin_name.to_string(),
            user_id: user_id.to_string(),
            req_json: req_json.to_string(),
            plugin_id,
            response_tx: tx,
        };
        
        worker.send(msg).map_err(|e| format!("Worker unavailable: {}", e))?;
        
        // Timeout de 15 segundos para RPC calls
        match tokio::time::timeout(std::time::Duration::from_secs(15), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("Worker response channel closed".to_string()),
            Err(_) => Err("RPC execution timeout".to_string()),
        }
    }

    /// Ejecuta lifecycle hook (fire-and-forget)
    pub fn execute_lifecycle(
        &self,
        plugin_script: &str,
        hook_name: &str,
        plugin_name: &str,
        user_id: &str,
        event_data_json: &str,
        plugin_id: uuid::Uuid,
        state: AppState,
    ) {
        let worker = self.get_next_worker();
        
        let msg = WorkerMessage::ExecuteLifecycle {
            id: Uuid::new_v4().to_string(),
            plugin_script: plugin_script.to_string(),
            hook_name: hook_name.to_string(),
            plugin_name: plugin_name.to_string(),
            user_id: user_id.to_string(),
            event_data_json: event_data_json.to_string(),
            plugin_id,
        };
        
        let _ = worker.send(msg); // Fire-and-forget
    }

    /// Obtiene estadísticas de todos los workers
    pub async fn get_stats(&self) -> Vec<WorkerStats> {
        let mut stats = Vec::new();
        
        for worker in &self.workers {
            let (tx, rx) = oneshot::channel();
            if worker.send(WorkerMessage::GetStats { response_tx: tx }).is_ok() {
                if let Ok(stat) = rx.await {
                    stats.push(stat);
                }
            }
        }
        
        stats
    }

    /// Detiene todos los workers
    pub fn shutdown(self) -> Vec<thread::Result<()>> {
        self.workers.into_iter()
            .map(|w| w.shutdown())
            .collect()
    }
}

/// Estado global del worker pool
static WORKER_POOL: std::sync::OnceLock<Arc<PluginWorkerPool>> = std::sync::OnceLock::new();

/// Obtiene el worker pool global
pub fn get_worker_pool() -> Option<Arc<PluginWorkerPool>> {
    WORKER_POOL.get().cloned()
}

// ── Sistema de Permisos Declarativos ─────────────────────────────────────────────

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
    pub plugin_id: Uuid,
    pub tables: std::collections::HashMap<String, TablePermission>,
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
    user_id: Uuid,
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
                if let Ok(uuid) = s.parse::<uuid::Uuid>() {
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
pub async fn clear_plugin_permissions_cache(plugin_id: Uuid) {
    if let Some(cache) = get_permissions_cache() {
        let mut perms_cache = cache.write().await;
        perms_cache.remove(&plugin_id);
    }
}
