use axum::{extract::State, response::Json};
use serde_json::json;
use crate::AppState;

pub async fn health_check(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db.pool).await.is_ok();

    let redis_ok: bool = if let Ok(mut guard) = state.redis.get_conn().await {
        if let Some(conn) = guard.as_mut() {
            redis::cmd("PING")
                .query_async::<_, String>(conn)
                .await
                .map(|r| r == "PONG")
                .unwrap_or(false)
        } else { false }
    } else { false };

    let status = if db_ok && redis_ok { "healthy" } else { "degraded" };

    Json(json!({
        "status": status,
        "services": {
            "database": if db_ok { "up" } else { "down" },
            "redis": if redis_ok { "up" } else { "down" },
        },
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": chrono::Utc::now(),
    }))
}
