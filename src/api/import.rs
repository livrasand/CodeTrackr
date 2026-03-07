use axum::{
    extract::State,
    response::Json,
    http::StatusCode,
};
use serde_json::json;
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::{AppState, auth::AuthenticatedUser, models::HeartbeatRequest};

pub async fn import_heartbeats(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(bodies): axum::Json<Vec<HeartbeatRequest>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if bodies.len() > 1000 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Maximum 1000 heartbeats per import request"}))));
    }

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
               ON CONFLICT DO NOTHING"#,
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
        .bind(body.duration.unwrap_or(60))
        .bind(body.is_write.unwrap_or(false))
        .bind(&body.editor)
        .bind(&body.os)
        .bind(&body.machine)
        .bind(recorded_at)
        .execute(&state.db.pool)
        .await;

        inserted += 1;
    }

    Ok(Json(json!({ "status": "ok", "inserted": inserted })))
}
