use axum::{
    extract::State,
    response::Json,
    http::StatusCode,
};
use serde_json::json;
use uuid::Uuid;
use chrono::Utc;

use crate::{AppState, auth::AuthenticatedUser, models::*};

pub async fn create_heartbeat(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<HeartbeatRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let recorded_at = body.time
        .and_then(|t| DateTime::from_timestamp(t as i64, 0))
        .unwrap_or_else(Utc::now);

    let duration = body.duration.unwrap_or(30);

    sqlx::query(
        r#"INSERT INTO heartbeats
           (id, user_id, project, file, language, branch, commit, workspace_root, package_path,
            duration_seconds, is_write, editor, os, machine, recorded_at, created_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,NOW())"#
    )
    .bind(Uuid::new_v4())
    .bind(user.id)
    .bind(&body.project)
    .bind(&body.file)
    .bind(&body.language)
    .bind(&body.branch)
    .bind(&body.commit)
    .bind(&body.workspace_root)
    .bind(&body.package_path)
    .bind(duration)
    .bind(body.is_write.unwrap_or(false))
    .bind(&body.editor)
    .bind(&body.os)
    .bind(&body.machine)
    .bind(recorded_at)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    // Publish real-time update via broadcast channel (forwarded by Redis subscriber to WS clients)
    let update = json!({
        "type": "heartbeat",
        "user_id": user.id.to_string(),
        "project": body.project,
        "language": body.language,
        "ts": Utc::now().timestamp(),
    });
    crate::realtime::ws_handler::publish(update.to_string());

    // Also publish to Redis (only using ZINCRBY for leaderboards since Leapcell Serverless Redis disables PUBLISH/SUBSCRIBE)
    {
        let week_key = format!("lb:week:{}", chrono::Utc::now().format("%Y-W%W"));
        let lang_key = body.language.as_ref().map(|lang| {
            format!("lb:lang:{}:{}", lang.to_lowercase(), chrono::Utc::now().format("%Y-W%W"))
        });

        if let Ok(mut guard) = state.redis.get_conn().await {
            if let Some(conn) = guard.as_mut() {
                let _: Result<(), _> = redis::cmd("ZINCRBY")
                    .arg(&week_key)
                    .arg(duration as i64)
                    .arg(user.id.to_string())
                    .query_async(conn)
                    .await;

                if let Some(lk) = lang_key {
                    let _: Result<(), _> = redis::cmd("ZINCRBY")
                        .arg(&lk)
                        .arg(duration as i64)
                        .arg(user.id.to_string())
                        .query_async(conn)
                        .await;
                }
            }
        }
    }

    Ok(Json(json!({"status": "ok", "recorded_at": recorded_at})))
}

pub async fn create_heartbeats_bulk(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(bodies): axum::Json<Vec<HeartbeatRequest>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut inserted = 0usize;

    for body in bodies {
        let recorded_at = body.time
            .and_then(|t| DateTime::from_timestamp(t as i64, 0))
            .unwrap_or_else(Utc::now);

        let _ = sqlx::query(
            r#"INSERT INTO heartbeats
               (id, user_id, project, file, language, branch, commit, workspace_root, package_path,
                duration_seconds, is_write, editor, os, machine, recorded_at, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,NOW())
               ON CONFLICT DO NOTHING"#
        )
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind(&body.project)
        .bind(&body.file)
        .bind(&body.language)
        .bind(&body.branch)
        .bind(&body.commit)
        .bind(&body.workspace_root)
        .bind(&body.package_path)
        .bind(body.duration.unwrap_or(30))
        .bind(body.is_write.unwrap_or(false))
        .bind(&body.editor)
        .bind(&body.os)
        .bind(&body.machine)
        .bind(recorded_at)
        .execute(&state.db.pool)
        .await;

        inserted += 1;
    }

    Ok(Json(json!({"status": "ok", "inserted": inserted})))
}

use chrono::DateTime;
