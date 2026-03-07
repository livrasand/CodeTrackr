use axum::{
    extract::{State, Query},
    response::{Json, Response, IntoResponse},
    http::{StatusCode, header},
};
use serde::Deserialize;
use serde_json::json;
use chrono::{DateTime, Utc};

use crate::{AppState, auth::AuthenticatedUser};

#[derive(Deserialize)]
pub struct ExportQuery {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct HbRow {
    recorded_at: Option<DateTime<Utc>>,
    project: Option<String>,
    file: Option<String>,
    language: Option<String>,
    branch: Option<String>,
    duration_seconds: Option<i32>,
    is_write: Option<bool>,
    editor: Option<String>,
}

async fn fetch_heartbeats(
    state: &AppState,
    user_id: uuid::Uuid,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<Vec<HbRow>, sqlx::Error> {
    let s = start.unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    let e = end.unwrap_or_else(Utc::now);

    sqlx::query_as::<_, HbRow>(
        r#"SELECT recorded_at, project, file, language, branch,
                  duration_seconds, is_write, editor
           FROM heartbeats WHERE user_id = $1 AND recorded_at BETWEEN $2 AND $3
           ORDER BY recorded_at ASC"#,
    )
    .bind(user_id).bind(s).bind(e)
    .fetch_all(&state.db.pool)
    .await
}

pub async fn export_json(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let rows = fetch_heartbeats(&state, user.id, q.start, q.end)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let data: Vec<serde_json::Value> = rows.into_iter().map(|r| {
        json!({
            "time": r.recorded_at,
            "project": r.project,
            "file": r.file,
            "language": r.language,
            "branch": r.branch,
            "duration": r.duration_seconds,
            "is_write": r.is_write,
            "editor": r.editor,
        })
    }).collect();

    let body = serde_json::to_string_pretty(&json!({"heartbeats": data, "exported_at": Utc::now()}))
        .unwrap_or_default();

    Ok((
        [(header::CONTENT_DISPOSITION, "attachment; filename=\"codetrackr_export.json\""),
         (header::CONTENT_TYPE, "application/json")],
        body,
    ).into_response())
}

pub async fn export_csv(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let rows = fetch_heartbeats(&state, user.id, q.start, q.end)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let mut csv = String::from("time,project,file,language,branch,duration_seconds,is_write,editor\n");
    for r in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            r.recorded_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            r.project.unwrap_or_default(),
            r.file.unwrap_or_default(),
            r.language.unwrap_or_default(),
            r.branch.unwrap_or_default(),
            r.duration_seconds.unwrap_or(0),
            r.is_write.unwrap_or(false),
            r.editor.unwrap_or_default(),
        ));
    }

    Ok((
        [(header::CONTENT_DISPOSITION, "attachment; filename=\"codetrackr_export.csv\""),
         (header::CONTENT_TYPE, "text/csv")],
        csv,
    ).into_response())
}
