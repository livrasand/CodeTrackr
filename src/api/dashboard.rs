use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{auth::AuthenticatedUser, AppState, error_handling};

/// POST /api/v1/dashboard/order
/// Saves the user's preferred dashboard panel order
pub async fn save_dashboard_order(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Json(payload): Json<SaveOrderPayload>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if payload.panel_names.len() > 100 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Too many panels (max 100)"})),
        ));
    }

    // Start transaction
    let mut tx = state.db.pool.begin().await
        .map_err(|e| error_handling::handle_database_error(e))?;

    // Delete existing order for this user
    sqlx::query("DELETE FROM user_dashboard_order WHERE user_id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| error_handling::handle_database_error(e))?;

    // Insert new order
    for (i, panel_name) in payload.panel_names.iter().enumerate() {
        sqlx::query(
            "INSERT INTO user_dashboard_order (user_id, panel_name, position) VALUES ($1, $2, $3)"
        )
        .bind(user.id)
        .bind(panel_name)
        .bind(i as i32)
        .execute(&mut *tx)
        .await
        .map_err(|e| error_handling::handle_database_error(e))?;
    }

    // Commit
    tx.commit().await
        .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct SaveOrderPayload {
    panel_names: Vec<String>,
}
