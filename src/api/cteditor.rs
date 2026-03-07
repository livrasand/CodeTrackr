use axum::{extract::State, response::{Html, IntoResponse}, Json};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use crate::{AppState, auth::AuthenticatedUser};

pub async fn serve_editor() -> impl IntoResponse {
    match tokio::fs::read_to_string("static/cteditor.html").await {
        Ok(content) => Html(content).into_response(),
        Err(_) => Html("<h1>Editor not found</h1>").into_response(),
    }
}

#[derive(Deserialize)]
pub struct RunRequest {
    pub code: String,
    pub trigger: String,
    pub rpc_body: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct RunResponse {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub async fn run_plugin(
    AuthenticatedUser(_user): AuthenticatedUser,
    State(_state): State<AppState>,
    Json(body): Json<RunRequest>,
) -> Json<RunResponse> {
    let trigger = if body.trigger.starts_with("rpc:") {
        body.trigger.clone()
    } else {
        match body.trigger.as_str() {
            "on_heartbeat" | "on_tick" | "on_install" => body.trigger.clone(),
            _ => "on_heartbeat".to_string(),
        }
    };

    let code = body.code.clone();
    let rpc_body = body.rpc_body.clone();

    // Run the QuickJS sandbox in a blocking thread (rquickjs is not Send)
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            run_in_quickjs(&code, &trigger, rpc_body.as_ref())
        }),
    ).await;

    match result {
        Ok(Ok(resp)) => Json(resp),
        Ok(Err(e)) => Json(RunResponse {
            success: false,
            stdout: String::new(),
            stderr: format!("Sandbox thread error: {e}"),
        }),
        Err(_) => Json(RunResponse {
            success: false,
            stdout: String::new(),
            stderr: "Plugin execution timed out (15s limit)".to_string(),
        }),
    }
}

/// Executes the plugin code inside an embedded QuickJS sandbox.
/// The user code has NO access to filesystem, network, or process — only
/// the mock ctx (db/redis) and log/warn/error defined in the script itself.
fn run_in_quickjs(code: &str, trigger: &str, rpc_body: Option<&serde_json::Value>) -> RunResponse {
    use rquickjs::{Context, Runtime};

    let rt = match Runtime::new() {
        Ok(r) => r,
        Err(e) => return RunResponse {
            success: false,
            stdout: String::new(),
            stderr: format!("Failed to create JS runtime: {e}"),
        },
    };

    rt.set_memory_limit(16 * 1024 * 1024);
    rt.set_max_stack_size(512 * 1024);

    let ctx = match Context::full(&rt) {
        Ok(c) => c,
        Err(e) => return RunResponse {
            success: false,
            stdout: String::new(),
            stderr: format!("Failed to create JS context: {e}"),
        },
    };

    let output: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let out_clone = Arc::clone(&output);

    let rpc_body_json = rpc_body
        .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "null".to_string()))
        .unwrap_or_else(|| "null".to_string());

    let script = build_sandbox_script(code, trigger, &rpc_body_json);

    let js_error: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);

    let eval_result = ctx.with(|ctx_ref| -> Result<(), rquickjs::Error> {
        // Expone __push_log(msg: String) — log/warn/error JS llaman a esto
        let func = rquickjs::Function::new(ctx_ref.clone(), move |msg: String| {
            out_clone.lock().unwrap().push(msg);
        })?;
        ctx_ref.globals().set("__push_log", func)?;

        match ctx_ref.eval::<rquickjs::Value, _>(script.as_bytes().to_vec()) {
            Ok(_) => Ok(()),
            Err(rquickjs::Error::Exception) => {
                // Extraer el mensaje real de la excepción JS
                let ex = ctx_ref.catch();
                let msg = if let Some(obj) = ex.as_object() {
                    obj.get::<_, String>("message").unwrap_or_else(|_| format!("{:?}", ex))
                } else if let Some(s) = ex.as_string() {
                    s.to_string().unwrap_or_else(|_| "Unknown exception".to_string())
                } else {
                    format!("{:?}", ex)
                };
                *js_error.borrow_mut() = Some(msg);
                Err(rquickjs::Error::Exception)
            }
            Err(e) => Err(e),
        }
    });

    // Drenar jobs pendientes del event loop (async/await) — fuera de ctx.with
    loop {
        match rt.execute_pending_job() {
            Ok(true) => continue,
            Ok(false) => break,
            Err(_) => break,
        }
    }

    let stdout = output.lock().unwrap().join("\n");

    match eval_result {
        Ok(()) => RunResponse { success: true, stdout, stderr: String::new() },
        Err(_) => {
            let stderr = js_error.into_inner()
                .unwrap_or_else(|| "Unknown error".to_string());
            RunResponse { success: false, stdout, stderr }
        }
    }
}

/// Builds the complete JS script that will run inside the QuickJS sandbox.
/// Includes mock ctx (db/redis), mock heartbeat, user code, and the trigger dispatch.
/// The user code is injected as a plain string — QuickJS tiene su propio scope aislado
/// sin acceso a globalThis.process, require, fs, net, etc.
fn build_sandbox_script(code: &str, trigger: &str, rpc_body_json: &str) -> String {
    let handler = if trigger.starts_with("rpc:") {
        trigger[4..].to_string()
    } else {
        String::new()
    };

    let dispatch = if trigger.starts_with("rpc:") {
        format!(
            r#"
(async () => {{
  if (typeof endpoints === 'undefined' || typeof endpoints["{handler}"] !== 'function') {{
    error('Handler "{handler}" not found in endpoints object');
    return;
  }}
  const _req = {rpc_body_json};
  try {{
    const _result = await endpoints["{handler}"](ctx, _req);
    log(JSON.stringify(_result ?? {{ ok: true }}));
  }} catch(e) {{
    error(e && e.message ? e.message : String(e));
  }}
}})();
"#,
            handler = handler,
            rpc_body_json = rpc_body_json,
        )
    } else {
        let call = match trigger {
            "on_heartbeat" => "await on_heartbeat(ctx, heartbeat);",
            "on_tick"      => "await on_tick(ctx);",
            "on_install"   => "await on_install(ctx);",
            _              => "await on_heartbeat(ctx, heartbeat);",
        };
        format!(
            r#"
(async () => {{
  try {{
    {call}
  }} catch(e) {{
    error(e && e.message ? e.message : String(e));
  }}
}})();
"#,
            call = call,
        )
    };

    format!(
        r#"
// ── Helpers de logging (usan __push_log expuesta desde Rust) ─────────────────
function log() {{ __push_log(Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function warn() {{ __push_log('[WARN] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}
function error() {{ __push_log('[ERROR] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }}

// ── Mock context (sandbox — sin acceso real a DB ni red) ─────────────────────
const ctx = {{
  user_id: "00000000-0000-0000-0000-000000000001",
  db: {{
    query: function(sql, params) {{
      log("[DB] query: " + sql.trim().substring(0, 80) + (sql.length > 80 ? "..." : "") + " params=" + JSON.stringify(params));
      return Promise.resolve([{{ total: 42, count: 10, id: "mock-id", value: "mock" }}]);
    }}
  }},
  redis: {{
    get:  function(key)        {{ log("[Redis] GET " + key); return Promise.resolve(null); }},
    set:  function(key, value) {{ log("[Redis] SET " + key + " = " + JSON.stringify(value)); return Promise.resolve(); }},
    incr: function(key)        {{ log("[Redis] INCR " + key); return Promise.resolve(1); }},
    del:  function(key)        {{ log("[Redis] DEL " + key); return Promise.resolve(); }},
  }},
  config: {{ base_url: "https://codetrackr.leapcell.app" }}
}};

const heartbeat = {{
  project: "my-project",
  language: "Rust",
  duration: 30,
  file: "src/main.rs",
  branch: "main",
  editor: "vscode",
  os: "linux"
}};

// ── User plugin code ──────────────────────────────────────────────────────────
{code}

// ── Trigger dispatch ──────────────────────────────────────────────────────────
{dispatch}
"#,
        code = code,
        dispatch = dispatch,
    )
}
