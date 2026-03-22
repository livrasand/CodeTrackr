/// CodeTrackr Plugin System
///
/// Developers can implement the `Plugin` trait to add custom functionality:
/// - Additional API routes
/// - Custom dashboard panels (via JSON manifest)
/// - Scheduled tasks (via `on_tick`)
/// - Custom stats processors
///
/// # Example
/// ```rust
/// use codetrackr::plugins::{Plugin, PluginContext, PluginRouter};
/// use axum::Router;
/// use async_trait::async_trait;
///
/// pub struct MyPlugin;
///
/// #[async_trait]
/// impl Plugin for MyPlugin {
///     fn name(&self) -> &'static str { "my-plugin" }
///     fn version(&self) -> &'static str { "0.1.0" }
///     fn description(&self) -> &'static str { "Does something cool" }
///
///     fn routes(&self) -> Option<Router<PluginContext>> {
///         Some(Router::new().route("/hello", axum::routing::get(|| async { "Hello from plugin!" })))
///     }
///
///     fn dashboard_manifest(&self) -> Option<serde_json::Value> {
///         Some(serde_json::json!({
///             "panel": "my-panel",
///             "title": "My Plugin Panel",
///             "icon": "🔌",
///             "api_endpoint": "/api/v1/plugins/my-plugin/hello",
///             "widget_type": "chart",   // "chart" | "table" | "counter" | "custom"
///         }))
///     }
/// }
/// ```

use axum::Router;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::RwLock;

use crate::AppState;

/// Shared context passed to all plugins
pub type PluginContext = AppState;

/// The Plugin trait — implement this to create a plugin
#[async_trait]
#[allow(dead_code)]
pub trait Plugin: Send + Sync + 'static {
    /// Unique plugin identifier (kebab-case)
    fn name(&self) -> &'static str;

    /// Semver version string
    fn version(&self) -> &'static str;

    /// Human-readable description
    fn description(&self) -> &'static str;

    /// Optional: register additional API routes under /api/v1/plugins/{name}/
    fn routes(&self) -> Option<Router<PluginContext>> {
        None
    }

    /// Optional: declare a dashboard UI panel
    /// Returns a JSON manifest that the frontend will render
    fn dashboard_manifest(&self) -> Option<Value> {
        None
    }

    /// Optional: called every 30 seconds for background tasks
    async fn on_tick(&self, _ctx: &PluginContext) {}

    /// Optional: called when a heartbeat is ingested
    async fn on_heartbeat(&self, _ctx: &PluginContext, _heartbeat: &Value) {}

    /// Optional: metadata/settings schema for this plugin
    fn settings_schema(&self) -> Option<Value> {
        None
    }
}

/// Plugin registry — holds all registered plugins
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self { plugins: Vec::new() }
    }

    #[allow(dead_code)]
    pub fn register<P: Plugin>(&mut self, plugin: P) {
        tracing::info!(
            "🔌 Plugin registered: {} v{} — {}",
            plugin.name(), plugin.version(), plugin.description()
        );
        self.plugins.push(Arc::new(plugin));
    }

    pub fn plugins(&self) -> &[Arc<dyn Plugin>] {
        &self.plugins
    }

    /// Build a combined router for all plugin API routes
    pub fn build_router(&self) -> Router<PluginContext> {
        let mut combined = Router::new();
        for plugin in &self.plugins {
            if let Some(router) = plugin.routes() {
                combined = combined.nest(&format!("/{}", plugin.name()), router);
            }
        }
        combined
    }

    /// Returns all dashboard panel manifests from plugins
    pub fn dashboard_manifests(&self) -> Vec<Value> {
        self.plugins.iter()
            .filter_map(|p| p.dashboard_manifest())
            .collect()
    }

    /// Returns plugin metadata list
    pub fn list(&self) -> Vec<Value> {
        self.plugins.iter().map(|p| serde_json::json!({
            "name": p.name(),
            "version": p.version(),
            "description": p.description(),
            "has_routes": p.routes().is_some(),
            "has_dashboard_panel": p.dashboard_manifest().is_some(),
            "settings_schema": p.settings_schema(),
        })).collect()
    }
}

/// Global plugin registry (thread-safe)
pub static REGISTRY: OnceLock<RwLock<PluginRegistry>> = OnceLock::new();

/// Initialize the global registry
pub fn get_registry() -> &'static RwLock<PluginRegistry> {
    REGISTRY.get_or_init(|| {
        RwLock::new(PluginRegistry::new())
    })
}

/// Register a plugin into the global registry
#[allow(dead_code)]
pub async fn register<P: Plugin>(plugin: P) {
    get_registry().write().await.register(plugin);
}

/// Start the plugin tick loop (every 30s)
pub fn start_tick_loop(ctx: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let registry = get_registry().read().await;
            for plugin in registry.plugins() {
                let p = Arc::clone(plugin);
                let c = ctx.clone();
                tokio::spawn(async move {
                    p.on_tick(&c).await;
                });
            }
        }
    });
}

// ── API handler to list plugins ───────────────────────────────────────────────

use axum::{
    extract::State,
    response::Json,
    http::StatusCode,
};

use crate::auth::AuthenticatedUser;

/// `GET /api/v1/plugins` — lists all compiled plugins registered in the global [`REGISTRY`].
///
/// Returns a JSON object with `plugins` (metadata array) and `count`. Only
/// compiled plugins (registered via [`register`]) appear here; store plugins
/// installed by individual users are served through the store API.
pub async fn list_plugins(
    State(_state): State<AppState>,
) -> Json<serde_json::Value> {
    let registry = get_registry().read().await;
    Json(serde_json::json!({
        "plugins": registry.list(),
        "count": registry.plugins().len(),
    }))
}

/// `GET /api/v1/plugins/manifests` — returns dashboard panel manifests for the authenticated user.
///
/// Combines two sources:
/// 1. **Compiled panels** — declared via [`Plugin::dashboard_manifest`] in the global registry.
/// 2. **Store panels** — plugins installed by this user from the plugin store, ordered by
///    the user's custom `user_dashboard_order` position.
///
/// Banned or unpublished store plugins are excluded. The response shape is
/// `{ "panels": [...] }` where each entry carries the fields expected by the
/// frontend widget renderer (`panel`, `title`, `icon`, `widget_type`, `api_endpoint`, etc.).
pub async fn get_dashboard_manifests(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Panels from compiled plugins (REGISTRY)
    let compiled_panels = {
        let registry = get_registry().read().await;
        registry.dashboard_manifests()
    };

    // 2. Panels from store plugins installed by this user (dynamic, no recompile needed)
    #[derive(sqlx::FromRow)]
    struct InstalledPanelRow {
        id: uuid::Uuid,
        name: String,
        display_name: String,
        icon: Option<String>,
        widget_type: Option<String>,
        api_endpoint: Option<String>,
        version: String,
        script: Option<String>,
        accepted_version: Option<String>,
        accepted_script: Option<String>,
    }

    let store_panels: Vec<Value> = sqlx::query_as::<_, InstalledPanelRow>(
        r#"SELECT p.id, p.name, p.display_name, p.icon, p.widget_type, p.api_endpoint,
                  p.version, p.script, i.accepted_version, i.accepted_script
           FROM plugin_store p
           JOIN installed_plugins i ON p.id = i.plugin_id
           LEFT JOIN user_dashboard_order u ON u.user_id = $1 AND u.panel_name = p.name
           WHERE i.user_id = $1 AND p.is_banned = false AND p.is_published = true
           ORDER BY u.position ASC NULLS LAST, p.name ASC"#,
    )
    .bind(user.id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("Error fetching dashboard panels for user {}: {}", user.id, e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "Failed to load dashboard panels"})))
    })?
    .into_iter()
    .map(|p| serde_json::json!({
        "panel": p.name,
        "title": p.display_name,
        "icon": p.icon.unwrap_or_else(|| "🔌".to_string()),
        "widget_type": p.widget_type.unwrap_or_else(|| "counter".to_string()),
        "api_endpoint": p.api_endpoint,
        "plugin_id": p.id,
        "version": p.version,
        "script": p.script,
        "accepted_version": p.accepted_version,
        "accepted_script": p.accepted_script,
    }))
    .collect();

    let mut panels = compiled_panels;
    panels.extend(store_panels);

    Ok(Json(serde_json::json!({ "panels": panels })))
}

