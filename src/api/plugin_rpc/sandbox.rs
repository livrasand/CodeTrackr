//! QuickJS sandbox execution environment for store plugins.
//!
//! Each plugin RPC call and lifecycle hook runs inside an isolated [QuickJS] runtime
//! (via `rquickjs`) within dedicated worker threads. The sandbox provides a controlled
//! JavaScript context with:
//!
//! - **`ctx.db.query(sql, params)`** — executes SQL validated by [`super::sql_validator`]
//! - **`ctx.redis.*`** — namespaced Redis operations (`plugin:{name}:{user_id}:{key}`)
//! - **`ctx.user_id`** — the authenticated user's UUID as a string
//! - **`log()` / `warn()` / `error()`** — captured and emitted via `tracing::debug`
//!
//! # Execution model
//! QuickJS runs synchronously in dedicated worker threads with real communication via
//! oneshot channels. The main thread delegates execution to the worker pool and waits
//! for results, providing true synchronous behavior without blocking the async runtime.
//!
//! # Resource limits
//! Every runtime is created with a 16 MiB heap limit and a 512 KiB stack limit.
//! RPC calls are wrapped in a 15-second Tokio timeout; lifecycle hooks in 30 seconds.
//!
//! [QuickJS]: https://bellard.org/quickjs/

use std::sync::{Arc, Mutex};
use serde_json::{json, Value};

use crate::{AppState, realtime::RedisPool};
use crate::services::plugin_permissions::execute_plugin_query_with_permissions;

/// Fuerza el prefijo de namespace en claves Redis para aislar plugins entre sí.
fn plugin_redis_key(plugin_name: &str, user_id: &str, key: &str) -> String {
    format!("plugin:{}:{}:{}", plugin_name, user_id, key)
}

/// Configura las funciones nativas (__db_query, __redis_*, etc.) en el contexto QuickJS.
/// Usa oneshot channels para sincronización real entre QuickJS y operaciones async.
fn setup_quickjs_context(
    ctx_ref: rquickjs::Ctx,
    db_pool: sqlx::PgPool,
    redis: RedisPool,
    plugin_name: &str,
    user_id: &str,
    plugin_id: uuid::Uuid,
) -> Result<(), rquickjs::Error> {
    let plugin_name_owned = plugin_name.to_string();
    let user_id_owned = user_id.to_string();
    let plugin_id_owned = plugin_id;

    // __db_query(sql, params_json) → JSON string de rows — síncrono con oneshot
    {
        let pool = db_pool.clone();
        let plugin_id_for_func = plugin_id_owned;
        let user_id_for_func = user_id_owned.parse::<uuid::Uuid>().unwrap_or_default();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |sql: String, params_json: String| -> String {
            let params: Vec<Value> = serde_json::from_str(&params_json).unwrap_or_default();
            let pool = pool.clone();
            let plugin_id = plugin_id_for_func;
            let user_id = user_id_for_func;
            
            // Crear oneshot channel para sincronización
            let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
            
            // Spawn async task que use permisos declarativos
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                match execute_plugin_query_with_permissions(&pool, plugin_id, user_id, &sql, &params).await {
                    Ok(rows) => {
                        let _ = tx.send(Ok(serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())));
                    },
                    Err(e) => {
                        tracing::warn!("Plugin db.query error: {e}");
                        let err_json = serde_json::json!({"__error": e}).to_string();
                        let _ = tx.send(Err(err_json));
                    }
                }
            });
            
            // Bloquear hasta recibir resultado (dentro del blocking thread)
            match handle.block_on(rx) {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => error,
                Err(_) => "{\"__error\": \"Async operation cancelled\"}".to_string(),
            }
        })?;
        ctx_ref.globals().set("__db_query", func)?;
    }

    // __redis_get(key) → valor como string JSON — síncrono con oneshot
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            
            // Crear oneshot channel para sincronización
            let (tx, rx) = tokio::sync::oneshot::channel::<String>();
            
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
                    let _ = tx.send(result);
                } else {
                    let _ = tx.send("null".to_string());
                }
            });
            
            // Bloquear hasta recibir resultado
            match handle.block_on(rx) {
                Ok(result) => result,
                Err(_) => "null".to_string(),
            }
        })?;
        ctx_ref.globals().set("__redis_get", func)?;
    }

    // __redis_set(key, value) — síncrono con oneshot
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String, value: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            
            let (tx, rx) = tokio::sync::oneshot::channel::<String>();
            
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                if let Ok(mut conn) = redis_ref.get_conn().await {
                    let _: () = redis::cmd("SET")
                        .arg(&namespaced).arg(&value)
                        .query_async(&mut conn)
                        .await
                        .unwrap_or(());
                    let _ = tx.send("\"ok\"".to_string());
                } else {
                    let _ = tx.send("\"error\"".to_string());
                }
            });
            
            match handle.block_on(rx) {
                Ok(result) => result,
                Err(_) => "\"error\"".to_string(),
            }
        })?;
        ctx_ref.globals().set("__redis_set", func)?;
    }

    // __redis_del(key) — síncrono con oneshot
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            
            let (tx, rx) = tokio::sync::oneshot::channel::<String>();
            
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                if let Ok(mut conn) = redis_ref.get_conn().await {
                    let _: () = redis::cmd("DEL")
                        .arg(&namespaced)
                        .query_async(&mut conn)
                        .await
                        .unwrap_or(());
                    let _ = tx.send("\"ok\"".to_string());
                } else {
                    let _ = tx.send("\"error\"".to_string());
                }
            });
            
            match handle.block_on(rx) {
                Ok(result) => result,
                Err(_) => "\"error\"".to_string(),
            }
        })?;
        ctx_ref.globals().set("__redis_del", func)?;
    }

    // __redis_incr(key) → i64 — síncrono con oneshot
    {
        let redis_ref = redis.clone();
        let pname = plugin_name_owned.clone();
        let uid = user_id_owned.clone();
        let func = rquickjs::Function::new(ctx_ref.clone(), move |key: String| -> String {
            let redis_ref = redis_ref.clone();
            let namespaced = plugin_redis_key(&pname, &uid, &key);
            
            let (tx, rx) = tokio::sync::oneshot::channel::<String>();
            
            let handle = tokio::runtime::Handle::current();
            handle.spawn(async move {
                if let Ok(mut conn) = redis_ref.get_conn().await {
                    let result = redis::cmd("INCR")
                        .arg(&namespaced)
                        .query_async::<_, i64>(&mut conn)
                        .await
                        .unwrap_or(0);
                    let _ = tx.send(result.to_string());
                } else {
                    let _ = tx.send("0".to_string());
                }
            });
            
            match handle.block_on(rx) {
                Ok(result) => result,
                Err(_) => "0".to_string(),
            }
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
/// Usa oneshot channels para sincronización real entre QuickJS y operaciones async.
pub fn run_rpc_in_quickjs(
    plugin_script: &str,
    handler: &str,
    plugin_name: &str,
    user_id: &str,
    req_json: &str,
    db_pool: sqlx::PgPool,
    redis: RedisPool,
    plugin_id: uuid::Uuid,
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

        // Configurar funciones de DB y Redis con sincronización real
        setup_quickjs_context(ctx_ref.clone(), db_pool, redis, plugin_name, user_id, plugin_id)?;

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
/// Usa oneshot channels para sincronización real sin carreras de condiciones.
pub fn run_lifecycle_hook_in_quickjs(
    plugin_script: &str,
    hook_name: &str,
    plugin_name: &str,
    user_id: &str,
    event_data_json: &str,
    state: AppState,
    plugin_id: uuid::Uuid,
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
        setup_quickjs_context(ctx_ref.clone(), db_pool, redis_pool, plugin_name, user_id, plugin_id)?;

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


    // Registrar logs del hook
    let logs = log_output.lock().unwrap();
    for line in logs.iter() {
        tracing::debug!("[lifecycle hook log] {line}");
    }
}

/// Construye el script JS completo para ejecutar en QuickJS.
/// ctx.db/redis son wrappers que llaman a las funciones Rust nativas expuestas.
#[allow(dead_code)]
pub fn build_rpc_sandbox(plugin_script: &str, handler: &str, _user_id: &str, req_json: &str) -> String {
    format!(
        r#"
// ── Helpers de logging ───────────────────────────────────────────────────────
function log() {{ __push_log(Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function warn() {{ __push_log('[WARN] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function error() {{ __push_log('[ERROR] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}

const req = {req_json};

// ── Context con DB y Redis reales (via Rust bridge) ──────────────────────────
const ctx = {{
  user_id: __user_id(),
  db: {{
    query: function(sql, params) {{
      const result = __db_query(sql, JSON.stringify(params || []));
      const parsed = JSON.parse(result);
      if (parsed && parsed.__error) throw new Error(parsed.__error);
      return Promise.resolve(parsed);
    }}
  }},
  redis: {{
    get:  function(key)        {{ 
      const result = __redis_get(key);
      return Promise.resolve(JSON.parse(result)); 
    }},
    set:  function(key, value) {{ 
      const result = __redis_set(key, JSON.stringify(value));
      return Promise.resolve(); 
    }},
    incr: function(key)        {{ 
      const result = __redis_incr(key);
      return Promise.resolve(parseInt(result)); 
    }},
    del:  function(key)        {{ 
      const result = __redis_del(key);
      return Promise.resolve(); 
    }}
  }},
  config: {{ base_url: "https://codetrackr.leapcell.app" }}
}};

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
      const parsed = JSON.parse(result);
      if (parsed && parsed.__error) throw new Error(parsed.__error);
      return Promise.resolve(parsed);
    }}
  }},
  redis: {{
    get:  function(key)        {{ 
      const result = __redis_get(key);
      return Promise.resolve(JSON.parse(result)); 
    }},
    set:  function(key, value) {{ 
      const result = __redis_set(key, JSON.stringify(value));
      return Promise.resolve(); 
    }},
    incr: function(key)        {{ 
      const result = __redis_incr(key);
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
  try {{
    var p = lifecycle["{hook_name}"](ctx, event);
    if (p && typeof p.then === 'function') {{
      p.then(function() {{
        // Hook completado exitosamente
      }}).catch(function(e) {{
        __push_log('[ERROR] Lifecycle hook {hook_name} failed: ' + (e && e.message ? e.message : String(e)));
      }});
    }}
  }} catch (e) {{
    __push_log('[ERROR] Lifecycle hook {hook_name} failed: ' + (e && e.message ? e.message : String(e)));
  }}
}})();
"#,
        event_data_json = event_data_json,
        plugin_script = plugin_script,
        hook_name = hook_name,
    )
}

/// Ejecuta RPC call usando ejecución directa (no utiliza worker pool)
#[allow(dead_code)]
pub fn execute_plugin_rpc(
    plugin_script: &str,
    handler: &str,
    plugin_name: &str,
    user_id: &str,
    req_json: &str,
    state: AppState,
    plugin_id: uuid::Uuid,
) -> Result<Value, String> {
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

/// Ejecuta lifecycle hook usando ejecución directa (no utiliza worker pool)
#[allow(dead_code)]
pub fn execute_plugin_lifecycle(
    plugin_script: &str,
    hook_name: &str,
    plugin_name: &str,
    user_id: &str,
    event_data_json: &str,
    state: AppState,
    plugin_id: uuid::Uuid,
) {
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
