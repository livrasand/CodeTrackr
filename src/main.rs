mod api;
mod auth;
mod db;
mod error_handling;
mod models;
mod realtime;
mod services;

use axum::{
    Router,
    routing::{get, post, delete},
};
use std::net::SocketAddr;
use tower_http::{
    cors::CorsLayer,
    trace::TraceLayer,
    compression::CompressionLayer,
};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use db::Database;
use realtime::RedisPool;
use services::plugins;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub redis: RedisPool,
    pub config: AppConfig,
    pub exchange_codes: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, String>>>,
}

#[derive(Clone)]
pub struct AppConfig {
    pub github_client_id: String,
    pub github_client_secret: String,
    pub gitlab_client_id: String,
    pub gitlab_client_secret: String,
    pub jwt_secret: String,
    pub base_url: String,
    pub frontend_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "codetrackr=debug,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = AppConfig {
        github_client_id: std::env::var("GITHUB_CLIENT_ID").expect("GITHUB_CLIENT_ID must be set"),
        github_client_secret: std::env::var("GITHUB_CLIENT_SECRET").expect("GITHUB_CLIENT_SECRET must be set"),
        gitlab_client_id: std::env::var("GITLAB_CLIENT_ID").unwrap_or_default(),
        gitlab_client_secret: std::env::var("GITLAB_CLIENT_SECRET").unwrap_or_default(),
        jwt_secret: std::env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
        base_url: std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        frontend_url: std::env::var("FRONTEND_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
    };

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");

    // Retry DB connection at startup (useful for serverless cold starts or DNS propagation)
    let mut db = None;
    for i in 1..=5 {
        tracing::info!("Connecting to PostgreSQL (attempt {}/5)...", i);
        match Database::new(&database_url).await {
            Ok(database) => {
                db = Some(database);
                break;
            }
            Err(e) => {
                if i == 5 {
                    return Err(anyhow::anyhow!("Failed to connect to DB after 5 attempts: {}", e));
                }
                tracing::warn!("DB connection failed, retrying in 2s... Error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }
    let db = db.unwrap();
    db.migrate_with_url(&database_url).await?;

    tracing::info!("Connecting to Redis...");
    let redis = RedisPool::new(&redis_url).await?;

    let state = AppState { db, redis, config, exchange_codes: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())) };

    // Start plugin tick loop
    plugins::start_tick_loop(state.clone());

    // Redis pub/sub subscriber is disabled because Leapcell Serverless Redis does not support SUBSCRIBE.
    // Realtime events will flow seamlessly through the local Tokio broadcast channel instead.
    // realtime::start_redis_subscriber(redis_url.clone()).await;

    let frontend_url = state.config.frontend_url.clone();
    let cors = CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::predicate(
            move |origin: &axum::http::HeaderValue, _: &axum::http::request::Parts| {
                let Ok(s) = origin.to_str() else { return false; };
                // Permitir cualquier localhost/127.0.0.1 (cualquier puerto) + FRONTEND_URL configurado
                s.starts_with("http://localhost:")
                    || s.starts_with("https://localhost:")
                    || s.starts_with("http://127.0.0.1:")
                    || s.starts_with("https://127.0.0.1:")
                    || s == frontend_url
            },
        ))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            "X-API-Key".parse::<axum::http::HeaderName>().unwrap(),
        ])
        .allow_credentials(true)
        .max_age(std::time::Duration::from_secs(3600));

    // Build plugin router (must be done here, inside async context)
    let plugin_router = {
        let registry = plugins::get_registry().read().await;
        registry.build_router()
    };

    let app = Router::new()
        // ── API v1 ──────────────────────────────────────────────────────
        .nest("/api/v1", api_routes(state.clone(), plugin_router))
        // ── Auth ────────────────────────────────────────────────────────
        .nest("/auth", auth_routes(state.clone()))
        // ── WebSocket / Real-time ────────────────────────────────────────
        .route("/ws", get(realtime::ws_handler))
        // ── Static frontend ──────────────────────────────────────────────
        .nest_service("/static", tower_http::services::ServeDir::new("static"))
        .route("/docs", get(api::frontend::serve_docs))
        .route("/plugin-store", get(api::frontend::serve_plugin_store))
        .route("/cteditor", get(api::cteditor::serve_editor))
        .route("/u/:username", get(api::frontend::serve_index))
        // SPA fallback
        .fallback(api::frontend::serve_index)
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new());

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("🚀 CodeTrackr running on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}

fn api_routes(state: AppState, plugin_router: Router<AppState>) -> Router<AppState> {

    // Rate limiting for webhook only (more restrictive)
    let webhook_governor_conf = GovernorConfigBuilder::default()
        .per_second(10)
        .burst_size(5)
        .finish()
        .unwrap();
    let webhook_governor_layer = GovernorLayer {
        config: std::sync::Arc::new(webhook_governor_conf),
    };

    // General rate limiting
    let governor_conf = GovernorConfigBuilder::default()
        .per_second(200)
        .burst_size(50)
        .finish()
        .unwrap();
    let governor_layer = GovernorLayer {
        config: std::sync::Arc::new(governor_conf),
    };

    // Webhook gets its own router with its own restrictive rate limiting
    let webhook_router = Router::new()
        .route("/billing/webhook", post(api::billing::stripe_webhook))
        .with_state(state.clone())
        .layer(webhook_governor_layer);

    Router::new()
        // Heartbeat
        .route("/heartbeat", post(api::heartbeats::create_heartbeat))
        .route("/heartbeats", post(api::heartbeats::create_heartbeats_bulk))
        // Stats
        .route("/stats/summary", get(api::stats::get_summary))
        .route("/stats/public", get(api::stats::get_public_summary))
        .route("/stats/languages", get(api::stats::get_languages))
        .route("/stats/projects", get(api::stats::get_projects))
        .route("/stats/daily", get(api::stats::get_daily))
        .route("/stats/streaks", get(api::stats::get_streaks))
        .route("/stats/work-types", get(api::stats::get_work_types))
        .route("/stats/sessions", get(api::stats::get_sessions))
        // Leaderboards
        .route("/leaderboards/global", get(api::leaderboards::get_global))
        .route("/leaderboards/language/:lang", get(api::leaderboards::get_by_language))
        .route("/leaderboards/country/:country", get(api::leaderboards::get_by_country))
        // API Keys
        .route("/keys", get(api::keys::list_keys))
        .route("/keys", post(api::keys::create_key))
        .route("/keys/:id", delete(api::keys::delete_key))
        // Export
        .route("/export/json", get(api::export::export_json))
        .route("/export/csv", get(api::export::export_csv))
        // Import
        .route("/import", post(api::import::import_heartbeats))
        // User
        .route("/user/me", get(api::users::get_me))
        .route("/user/profile/update", post(api::users::update_profile))
        .route("/user/follow/:username", post(api::users::follow_user))
        .route("/user/unfollow/:username", delete(api::users::unfollow_user))
        .route("/user/following/:username", get(api::users::is_following))
        .route("/user/badge/:username/:lang", get(api::users::get_badge))
        .route("/user/profile/:username", get(api::users::get_public_profile))
        .route("/user/contact/:username", post(api::users::contact_dev))
        // Plugin editor sandbox
        .route("/cteditor/run", post(api::cteditor::run_plugin))
        // Plugin RPC — user-defined endpoints inside store plugin scripts
        .route("/plugins/:name/rpc/:handler", post(api::plugin_rpc::rpc_call))
        // WebSocket ticket (single-use, TTL 30s — evita exponer JWT en query string)
        .route("/ws-ticket", post(realtime::ws_handler::create_ws_ticket))
        // Health
        .route("/health", get(api::health::health_check))
        .route("/kaithheathcheck", get(api::health::health_check))
        // Plugin system
        .route("/plugins", get(plugins::list_plugins))
        .route("/plugins/panels", get(plugins::get_dashboard_manifests))
        .route("/dashboard/order", post(api::dashboard::save_dashboard_order))
        .nest("/plugins", plugin_router)
        // Theme store
        .route("/themes", get(api::themes::list_themes))
        .route("/themes/publish", post(api::themes::publish_theme))
        .route("/themes/install/:id", post(api::themes::install_theme))
        .route("/themes/uninstall/:id", delete(api::themes::uninstall_theme))
        .route("/themes/installed", get(api::themes::get_installed_themes))
        .route("/themes/active", get(api::themes::get_active_theme))
        .route("/themes/apply", post(api::themes::apply_theme))
        // Plugin store
        .route("/store", get(api::store::list_store_plugins))
        .route("/store/publish", post(api::store::publish_plugin))
        .route("/store/install/:id", post(api::store::install_plugin))
        .route("/store/uninstall/:id", delete(api::store::uninstall_plugin))
        .route("/store/installed", get(api::store::get_installed_plugins))
        .route("/store/plugin/:id/script", get(api::store::get_plugin_script))
        .route("/store/plugin/:id/accepted", get(api::store::get_plugin_accepted_version))
        .route("/store/plugin/:id/accept", post(api::store::accept_plugin_version))
        .route("/store/plugin/:id/detail", get(api::store::get_plugin_detail))
        .route("/store/plugin/:id/rate", post(api::store::rate_plugin))
        .route("/store/plugin/:id/review", post(api::store::review_plugin))
        .route("/store/plugin/:id/screenshot", post(api::store::add_screenshot))
        .route("/store/report/:id", post(api::store::report_plugin))
        .route("/store/my/:id", delete(api::store::author_delete_plugin))
        // Admin
        .route("/store/admin/plugins", get(api::store::admin_list_plugins))
        .route("/store/admin/ban/:id", post(api::store::admin_ban_plugin))
        .route("/store/admin/unban/:id", post(api::store::admin_unban_plugin))
        .route("/store/admin/delete/:id", delete(api::store::admin_delete_plugin))
        .route("/store/admin/reports", get(api::store::admin_list_reports))
        .route("/store/admin/reports/:id/resolve", post(api::store::admin_resolve_report))
        // Billing / Stripe (con rate limiting específico)
        .route("/billing/config", get(api::billing::get_billing_config))
        .route("/billing/status", get(api::billing::get_billing_status))
        .route("/billing/checkout", post(api::billing::create_checkout_session))
        .route("/billing/portal", post(api::billing::create_portal_session))
        .with_state(state)
        .layer(governor_layer)  // Rate limiting general para API
        .merge(webhook_router)
}


fn auth_routes(state: AppState) -> Router<AppState> {
    // Rate limiting for auth endpoints (more restrictive)
    let auth_governor_conf = GovernorConfigBuilder::default()
        .per_second(100)
        .burst_size(20)
        .finish()
        .unwrap();
    let auth_governor_layer = GovernorLayer {
        config: std::sync::Arc::new(auth_governor_conf),
    };

    Router::new()
        .route("/github", get(auth::github::github_login))
        .route("/github/callback", get(auth::github::github_callback))
        .route("/exchange", post(auth::github::exchange_code))
        .route("/gitlab", get(auth::gitlab::gitlab_login))
        .route("/gitlab/callback", get(auth::gitlab::gitlab_callback))
        .route("/anonymous/create", post(auth::anonymous::create_anonymous_account))
        .route("/anonymous/login", post(auth::anonymous::login_with_account_number))
        .route("/anonymous/verify", post(auth::anonymous::verify_account_number))
        .route("/logout", post(auth::logout))
        .with_state(state)
        .layer(auth_governor_layer)  // Rate limiting específico para auth
}
