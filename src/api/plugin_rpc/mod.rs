pub mod sql_validator;
pub mod sandbox;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde_json::{json, Value};
use futures::{stream::{self, StreamExt}};

use crate::{AppState, auth::AuthenticatedUser, error_handling};
use sandbox::run_rpc_in_quickjs;

/// Ejecuta hooks de lifecycle en plugins JavaScript instalados por el usuario.
/// Los plugins pueden exportar un objeto `lifecycle` con hooks como `on_user_register`, `on_heartbeat`, etc.
/// Los hooks se ejecutan con concurrencia limitada para evitar agotar recursos del servidor.
pub async fn execute_lifecycle_hooks(
    user_id: &uuid::Uuid,
    hook_name: &str,
    event_data: serde_json::Value,
    state: &AppState,
) {
    // Límite: máximo 3 plugins simultáneos por llamada (configurable via env var)
    let max_concurrent: usize = std::env::var("MAX_PLUGIN_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    // Obtener todos los plugins instalados por el usuario que tienen script
    let plugins: Vec<(String, String, uuid::Uuid)> = match sqlx::query_as::<_, (String, String, uuid::Uuid)>(
        r#"SELECT p.name, p.script, p.id
           FROM plugin_store p
           JOIN installed_plugins i ON p.id = i.plugin_id
           WHERE i.user_id = $1 AND p.is_banned = false AND p.is_published = true AND p.script IS NOT NULL"#,
    )
    .bind(user_id)
    .fetch_all(&state.db.pool)
    .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("[lifecycle] Error fetching plugins for user {}: {}", user_id, e);
            return;
        }
    };

    // Ejecutar hooks con concurrencia limitada usando buffer_unordered
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(30), // 30s timeout total
        stream::iter(plugins)
            .map(|(plugin_name, script, plugin_id)| {
                let event_data = event_data.clone();
                let state = state.clone();
                let user_id = *user_id;
                let hook_name = hook_name.to_string();

                async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        sandbox::run_lifecycle_hook_in_quickjs(
                            &script,
                            &hook_name,
                            &plugin_name,
                            &user_id.to_string(),
                            &serde_json::to_string(&event_data).unwrap_or_else(|_| "{}".to_string()),
                            state,
                            plugin_id,
                        );
                    }).await;
                }
            })
            // Aquí está la clave: solo max_concurrent futuros activos a la vez
            .buffer_unordered(max_concurrent)
            .collect::<()>(),
    ).await;
}

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
    struct PluginRow { 
        script: Option<String>,
        id: uuid::Uuid,
    }

    let row = sqlx::query_as::<_, PluginRow>(
        r#"SELECT p.script, p.id
           FROM plugin_store p
           JOIN installed_plugins i ON p.id = i.plugin_id
           WHERE p.name = $1 AND i.user_id = $2 AND p.is_banned = false AND p.is_published = true"#,
    )
    .bind(&plugin_name)
    .bind(user.id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found or not installed"}))))?;

    let script = row.script.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Plugin has no script"})))
    })?;
    
    let plugin_id = row.id;

    let req_body = body.map(|b| b.0).unwrap_or(Value::Null);
    let req_json = serde_json::to_string(&req_body).unwrap_or_else(|_| "null".to_string());

    // 2. Ejecutar en sandbox QuickJS con timeout de 15s
    let user_id_str = user.id.to_string();
    let db_pool = state.db.pool.clone();
    let redis_pool = state.redis.clone();
    let plugin_name_log = plugin_name.clone();
    let handler_safe_log = handler_safe.clone();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::task::spawn_blocking(move || {
            run_rpc_in_quickjs(&script, &handler_safe, &plugin_name, &user_id_str, &req_json, db_pool, redis_pool, plugin_id)
        }),
    ).await;

    match result {
        Ok(Ok(plugin_result)) => {
            match plugin_result {
                Ok(value) => Ok(Json(value)),
                Err(err_msg) => {
                    tracing::warn!("[plugin rpc] handler failed — plugin={} handler={} detail={}", plugin_name_log, handler_safe_log, err_msg);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Plugin handler failed", "detail": err_msg})),
                    ))
                }
            }
        },
        Ok(Err(join_err)) => {
            tracing::warn!("[plugin rpc] task join error — plugin={} handler={} detail={}", plugin_name_log, handler_safe_log, join_err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Plugin task failed", "detail": join_err.to_string()})),
            ))
        }
        Err(_) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(json!({"error": "Plugin handler timed out (15s limit)"})),
        )),
    }
}
