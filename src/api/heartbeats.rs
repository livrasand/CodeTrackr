use axum::{
    extract::State,
    response::Json,
    http::StatusCode,
};
use serde_json::json;
use uuid::Uuid;
use chrono::Utc;

use crate::{AppState, auth::AuthenticatedUser, models::*, error_handling};

pub async fn create_heartbeat(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<HeartbeatRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let recorded_at = body.time
        .and_then(|t| DateTime::from_timestamp(t as i64, 0))
        .unwrap_or_else(Utc::now);

    let duration = body.duration.unwrap_or(30);

    // Check if user is anonymous and scrub identifying metadata
    let is_anonymous = user.account_number.is_some();
    let (file, branch, commit, machine) = if is_anonymous {
        (None, None, None, None)
    } else {
        (body.file.as_deref(), body.branch.as_deref(), body.commit.as_deref(), body.machine.as_deref())
    };

    sqlx::query(
        r#"INSERT INTO heartbeats
           (id, user_id, project, file, language, branch, commit, workspace_root, package_path,
            duration_seconds, is_write, editor, os, machine, recorded_at, created_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,NOW())"#
    )
    .bind(Uuid::new_v4())
    .bind(user.id)
    .bind(&body.project)
    .bind(file)
    .bind(&body.language)
    .bind(branch)
    .bind(commit)
    .bind(&body.workspace_root)
    .bind(&body.package_path)
    .bind(duration)
    .bind(body.is_write.unwrap_or(false))
    .bind(&body.editor)
    .bind(&body.os)
    .bind(machine)
    .bind(recorded_at)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    // Ejecutar hooks de lifecycle para 'on_heartbeat'
    let event_data = json!({
        "user_id": user.id,
        "project": body.project,
        "file": file,
        "language": body.language,
        "branch": branch,
        "commit": commit,
        "workspace_root": body.workspace_root,
        "package_path": body.package_path,
        "duration_seconds": duration,
        "is_write": body.is_write.unwrap_or(false),
        "editor": body.editor,
        "os": body.os,
        "machine": machine,
        "recorded_at": recorded_at
    });
    crate::api::plugin_rpc::execute_lifecycle_hooks(&user.id, "on_heartbeat", event_data, &state).await;

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
        let lang_key = body.language.as_ref().and_then(|lang| {
            // Validate language contains only alphanumeric characters
            if lang.chars().all(|c| c.is_alphanumeric()) {
                Some(format!("lb:lang:{}:{}", lang.to_lowercase(), chrono::Utc::now().format("%Y-W%W")))
            } else {
                None
            }
        });

        if let Ok(mut conn) = state.redis.get_conn().await {
            let _: Result<(), _> = redis::cmd("ZINCRBY")
                .arg(&week_key)
                .arg(duration as i64)
                .arg(user.id.to_string())
                .query_async(&mut conn)
                .await;

            if let Some(lk) = lang_key {
                let _: Result<(), _> = redis::cmd("ZINCRBY")
                    .arg(&lk)
                    .arg(duration as i64)
                    .arg(user.id.to_string())
                    .query_async(&mut conn)
                    .await;
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
    
    // Check if user is anonymous and scrub identifying metadata
    let is_anonymous = user.account_number.is_some();

    for body in &bodies {
        let recorded_at = body.time
            .and_then(|t| DateTime::from_timestamp(t as i64, 0))
            .unwrap_or_else(Utc::now);

        let (file, branch, commit, machine) = if is_anonymous {
            (None, None, None, None)
        } else {
            (body.file.as_deref(), body.branch.as_deref(), body.commit.as_deref(), body.machine.as_deref())
        };

        if let Ok(result) = sqlx::query(
            r#"INSERT INTO heartbeats
               (id, user_id, project, file, language, branch, commit, workspace_root, package_path,
                duration_seconds, is_write, editor, os, machine, recorded_at, created_at)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,NOW())
               ON CONFLICT DO NOTHING"#
        )
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind(&body.project)
        .bind(file)
        .bind(&body.language)
        .bind(branch)
        .bind(commit)
        .bind(&body.workspace_root)
        .bind(&body.package_path)
        .bind(body.duration.unwrap_or(30))
        .bind(body.is_write.unwrap_or(false))
        .bind(&body.editor)
        .bind(&body.os)
        .bind(machine)
        .bind(recorded_at)
        .execute(&state.db.pool)
        .await
        {
            if result.rows_affected() > 0 {
                inserted += 1;
            }
        }
    }

    // Ejecutar hooks de lifecycle para 'on_heartbeats_bulk'
    let event_data = json!({
        "user_id": user.id,
        "count": inserted,
        "total_requested": bodies.len()
    });
    crate::api::plugin_rpc::execute_lifecycle_hooks(&user.id, "on_heartbeats_bulk", event_data, &state).await;

    Ok(Json(json!({"status": "ok", "inserted": inserted})))
}

use chrono::DateTime;
