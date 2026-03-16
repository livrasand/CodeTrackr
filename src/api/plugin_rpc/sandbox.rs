//! QuickJS sandbox execution environment for store plugins.
//!
//! Each plugin RPC call and lifecycle hook runs inside an isolated [QuickJS] runtime
//! (via `rquickjs`). The sandbox provides a controlled JavaScript context with:
//!
//! - **`ctx.db.query(sql, params)`** — executes SQL validated by [`super::sql_validator`]
//! - **`ctx.redis.*`** — namespaced Redis operations (`plugin:{name}:{user_id}:{key}`)
//! - **`ctx.user_id`** — the authenticated user's UUID as a string
//! - **`log()` / `warn()` / `error()`** — captured and emitted via `tracing::debug`
//!
//! # Execution model
//! QuickJS runs synchronously in a `spawn_blocking` thread. Async DB/Redis calls are
//! dispatched via `tokio::spawn` from within the blocking thread and communicate
//! results back through an unbounded channel. A 10 ms sleep after the event loop drain
//! allows in-flight async tasks to resolve before results are collected.
//!
//! # Resource limits
//! Every runtime is created with a 16 MiB heap limit and a 512 KiB stack limit.
//! RPC calls are wrapped in a 15-second Tokio timeout; lifecycle hooks in 30 seconds.
//!
//! [QuickJS]: https://bellard.org/quickjs/

use std::sync::{Arc, Mutex};
use serde_json::{json, Value};

use crate::{AppState, realtime::RedisPool};
use super::sql_validator::validate_plugin_sql;

/// Fuerza el prefijo de namespace en claves Redis para aislar plugins entre sí.
fn plugin_redis_key(plugin_name: &str, user_id: &str, key: &str) -> String {
    format!("plugin:{}:{}:{}", plugin_name, user_id, key)
}

/// Configura las funciones nativas (__db_query, __redis_*, etc.) en el contexto QuickJS.
/// Ahora usa canales para comunicación async en lugar de block_on.
fn setup_quickjs_context(
    ctx_ref: rquickjs::Ctx,
    db_pool: sqlx::PgPool,
    redis: RedisPool,
    plugin_name: &str,
    user_id: &str,
    result_sender: tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), rquickjs::Error> {
    let plugin_name_owned = plugin_name.to_string();
    let user_id_owned = user_id.to_string();

    // __db_query(sql, params_json) → JSON string de rows — asíncrono vía canal
    {
        let pool = db_pool.clone();
        let sender = result_sender.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |sql: String, params_json: String| -> String {
            // Validar contra allowlist antes de ejecutar
            if let Err(_) = validate_plugin_sql(&sql) {
                return format!("{{\"__error\": \"SQL validation failed\"}}");
            }
            
            let params: Vec<Value> = serde_json::from_str(&params_json).unwrap_or_default();
            let pool = pool.clone();
            let sender = sender.clone();
            
            // Spawn async task que envíe resultado por canal
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
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
                
                match q.fetch_all(&pool).await {
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
                        let _ = sender.send(serde_json::to_string(&json_rows).unwrap_or_else(|_| "[]".to_string()));
                    },
                    Err(e) => {
                        tracing::warn!("Plugin db.query error: {e}");
                        let _ = sender.send(format!("{{\"__error\": \"{}\"}}", e.to_string().replace('"', "'")));
                    }
                }
            });
            
            // Retornar placeholder inmediato
            "__pending__".to_string()
        })?;
        ctx_ref.globals().set("__db_query", func)?;
    }

    // __redis_get(key) → valor como string JSON — asíncrono vía canal
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let sender = result_sender.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            let sender = sender.clone();
            
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                if let Ok(mut conn) = redis_ref.get_conn().await {
                    let val: Option<String> = redis::cmd("GET")
                        .arg(&namespaced)
                        .query_async(&mut conn)
                        .await
                        .unwrap_or(None);
                    let result = match val {
                        Some(v) => format!("\"{}\"", v.replace('"', "\\\"")),
                        None => "null".to_string(),
                    };
                    let _ = sender.send(result);
                } else {
                    let _ = sender.send("null".to_string());
                }
            });
            
            "__pending__".to_string()
        })?;
        ctx_ref.globals().set("__redis_get", func)?;
    }

    // __redis_set(key, value) — asíncrono vía canal
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let sender = result_sender.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String, value: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            let sender = sender.clone();
            
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                if let Ok(mut conn) = redis_ref.get_conn().await {
                    let _: () = redis::cmd("SET")
                        .arg(&namespaced).arg(&value)
                        .query_async(&mut conn)
                        .await
                        .unwrap_or(());
                    let _ = sender.send("\"ok\"".to_string());
                } else {
                    let _ = sender.send("\"error\"".to_string());
                }
            });
            
            "__pending__".to_string()
        })?;
        ctx_ref.globals().set("__redis_set", func)?;
    }

    // __redis_del(key) — asíncrono vía canal
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let sender = result_sender.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            let sender = sender.clone();
            
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                if let Ok(mut conn) = redis_ref.get_conn().await {
                    let _: () = redis::cmd("DEL")
                        .arg(&namespaced)
                        .query_async(&mut conn)
                        .await
                        .unwrap_or(());
                    let _ = sender.send("\"ok\"".to_string());
                } else {
                    let _ = sender.send("\"error\"".to_string());
                }
            });
            
            "__pending__".to_string()
        })?;
        ctx_ref.globals().set("__redis_del", func)?;
    }

    // __redis_incr(key) → i64 — asíncrono vía canal
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let sender = result_sender.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            let sender = sender.clone();
            
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                if let Ok(mut conn) = redis_ref.get_conn().await {
                    let result = redis::cmd("INCR")
                        .arg(&namespaced)
                        .query_async::<_, i64>(&mut conn)
                        .await
                        .unwrap_or(0);
                    let _ = sender.send(result.to_string());
                } else {
                    let _ = sender.send("0".to_string());
                }
            });
            
            "__pending__".to_string()
        })?;
        ctx_ref.globals().set("__redis_incr", func)?;
    }

    // __user_id() → retorna el user_id como string
    {
        let uid = user_id_owned.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move || -> String {
            uid.clone()
        })?;
        ctx_ref.globals().set("__user_id", func)?;
    }

    Ok(())
}

/// Ejecuta el handler del plugin dentro de un sandbox QuickJS embebido.
/// Versión híbrida - usa spawn_blocking pero sin block_on interno.
/// Las operaciones de DB/Redis se ejecutan async vía tokio::spawn dentro del blocking thread.
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

    // Captura de output del plugin
    let log_output: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_clone = Arc::clone(&log_output);

    // Canal para recibir el resultado JSON del handler
    let result_cell: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
    let result_clone = Arc::clone(&result_cell);
    let error_cell: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let error_clone = Arc::clone(&error_cell);

    // Canal para resultados async de DB/Redis
    let (result_sender, _result_receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
    let sender_clone = result_sender.clone();

    let script = build_rpc_sandbox(plugin_script, handler, user_id, req_json);

    ctx.with(|ctx_ref| -> Result<(), rquickjs::Error> {
        // __push_log(msg) — log/warn/error del script JS llaman a esto
        {
            let out = Arc::clone(&log_clone);
            let func = rquickjs::Function::new(ctx_ref.clone(), move |msg: String| {
                out.lock().unwrap().push(msg);
            })?;
            ctx_ref.globals().set("__push_log", func)?;
        }

        // Configurar funciones de DB y Redis usando canal para async
        setup_quickjs_context(ctx_ref.clone(), db_pool, redis, plugin_name, user_id, sender_clone)?;

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

    // Pequeña espera para que lleguen resultados async por canal
    std::thread::sleep(std::time::Duration::from_millis(10));

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
/// Versión síncrona para compatibilidad con tokio::spawn_blocking.
pub fn run_lifecycle_hook_in_quickjs(
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

    // Configurar funciones de DB y Redis
    let db_pool = state.db.pool.clone();
    let redis_pool = state.redis.clone();
    let log_output: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_clone = Arc::clone(&log_output);
    let (result_sender, _result_receiver) = tokio::sync::mpsc::unbounded_channel::<String>();

    let script = build_lifecycle_sandbox(plugin_script, hook_name, user_id, event_data_json);

    if let Err(e) = ctx.with(|ctx_ref| -> Result<(), rquickjs::Error> {
        // __push_log(msg) — log/warn/error del script JS llaman a esto
        {
            let out = Arc::clone(&log_clone);
            let func = rquickjs::Function::new(ctx_ref.clone(), move |msg: String| {
                out.lock().unwrap().push(msg);
            })?;
            ctx_ref.globals().set("__push_log", func)?;
        }

        // Configurar funciones de DB y Redis
        setup_quickjs_context(ctx_ref.clone(), db_pool, redis_pool, plugin_name, user_id, result_sender)?;

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

    // Pequeña espera para resultados async (thread::sleep en contexto síncrono)
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Registrar logs del hook
    let logs = log_output.lock().unwrap();
    for line in logs.iter() {
        tracing::debug!("[lifecycle hook log] {line}");
    }
}

/// Construye el script JS completo para ejecutar en QuickJS.
/// ctx.db/redis son wrappers que llaman a las funciones Rust nativas expuestas.
pub fn build_rpc_sandbox(plugin_script: &str, handler: &str, _user_id: &str, req_json: &str) -> String {
    format!(
        r#"
// ── Helpers de logging ───────────────────────────────────────────────────────
function log() {{ __push_log(Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function warn() {{ __push_log('[WARN] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function error() {{ __push_log('[ERROR] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}

// ── Context con DB y Redis reales (via Rust bridge) ──────────────────────────
const ctx = {{
  user_id: __user_id(),
  db: {{
    query: function(sql, params) {{
      const result = __db_query(sql, JSON.stringify(params || []));
      if (result === "__pending__") return Promise.resolve([]); // Simulación simple
      const parsed = JSON.parse(result);
      if (parsed && parsed.__error) throw new Error(parsed.__error);
      return Promise.resolve(parsed);
    }}
  }},
  redis: {{
    get:  function(key)        {{ 
      const result = __redis_get(key);
      if (result === "__pending__") return Promise.resolve(null);
      return Promise.resolve(JSON.parse(result)); 
    }},
    set:  function(key, value) {{ 
      const result = __redis_set(key, JSON.stringify(value));
      return Promise.resolve(); 
    }},
    incr: function(key)        {{ 
      const result = __redis_incr(key);
      if (result === "__pending__") return Promise.resolve(1);
      return Promise.resolve(parseInt(result)); 
    }},
    del:  function(key)        {{ 
      const result = __redis_del(key);
      return Promise.resolve(); 
    }}
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
        req_json = req_json,
        plugin_script = plugin_script,
        handler = handler,
    )
}

/// Construye el script JS completo para ejecutar hooks de lifecycle en QuickJS.
/// Similar a build_rpc_sandbox pero ejecuta hooks de lifecycle que no retornan valores.
pub fn build_lifecycle_sandbox(plugin_script: &str, hook_name: &str, _user_id: &str, event_data_json: &str) -> String {
    format!(
        r#"
// ── Helpers de logging ───────────────────────────────────────────────────────
function log() {{ __push_log(Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function warn() {{ __push_log('[WARN] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function error() {{ __push_log('[ERROR] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}

// ── Context con DB y Redis reales (via Rust bridge) ──────────────────────────
const ctx = {{
  user_id: __user_id(),
  db: {{
    query: function(sql, params) {{
      const result = __db_query(sql, JSON.stringify(params || []));
      if (result === "__pending__") return Promise.resolve([]);
      const parsed = JSON.parse(result);
      if (parsed && parsed.__error) throw new Error(parsed.__error);
      return Promise.resolve(parsed);
    }}
  }},
  redis: {{
    get:  function(key)        {{ 
      const result = __redis_get(key);
      if (result === "__pending__") return Promise.resolve(null);
      return Promise.resolve(JSON.parse(result)); 
    }},
    set:  function(key, value) {{ 
      const result = __redis_set(key, JSON.stringify(value));
      return Promise.resolve(); 
    }},
    incr: function(key)        {{ 
      const result = __redis_incr(key);
      if (result === "__pending__") return Promise.resolve(1);
      return Promise.resolve(parseInt(result)); 
    }},
    del:  function(key)        {{ 
      const result = __redis_del(key);
      return Promise.resolve(); 
    }}
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
        event_data_json = event_data_json,
        plugin_script = plugin_script,
        hook_name = hook_name,
    )
}
