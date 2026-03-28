use axum::{
    extract::State,
    response::Json,
    http::StatusCode,
};
use serde_json::json;
use uuid::Uuid;
use chrono::Utc;

use crate::{
    AppState,
    auth::{AuthenticatedUser, generate_api_key, hash_api_key_with_secret},
    models::{ApiKey, ApiKeyResponse, ApiKeyCreated},
    error_handling,
};

pub async fn list_keys(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let keys = sqlx::query_as::<_, ApiKey>(
        "SELECT * FROM api_keys WHERE user_id = $1 ORDER BY created_at DESC"
    )
    .bind(user.id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    let response: Vec<ApiKeyResponse> = keys.into_iter().map(|k| ApiKeyResponse {
        id: k.id,
        name: k.name,
        key_prefix: k.key_prefix,
        last_used_at: k.last_used_at,
        created_at: k.created_at,
    }).collect();

    Ok(Json(json!({"keys": response})))
}

#[derive(serde::Deserialize)]
pub struct CreateKeyRequest {
    pub name: Option<String>,
}

pub async fn create_key(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<CreateKeyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Max 5 keys per user
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_keys WHERE user_id = $1"
    )
    .bind(user.id)
    .fetch_one(&state.db.pool)
    .await
    .unwrap_or(0);

    if count >= 5 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Maximum 5 API keys allowed"}))));
    }

    let key = generate_api_key();
    let key_hash = hash_api_key_with_secret(&key, &state.config.jwt_secret);
    let prefix = key[..12].to_string();
    let name = body.name.unwrap_or_else(|| format!("Key #{}", count + 1));
    let id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO api_keys (id, user_id, name, key_hash, key_prefix, created_at) VALUES ($1, $2, $3, $4, $5, NOW())"
    )
    .bind(id)
    .bind(user.id)
    .bind(&name)
    .bind(&key_hash)
    .bind(&prefix)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({
        "key": ApiKeyCreated {
            id,
            name,
            key,   // Only time the full key is shown
            created_at: Utc::now(),
        }
    })))
}

#[derive(serde::Deserialize)]
pub struct RenameKeyRequest {
    pub name: String,
}

pub async fn rename_key(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    axum::Json(body): axum::Json<RenameKeyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Name cannot be empty"}))));
    }

    let result = sqlx::query(
        "UPDATE api_keys SET name = $1 WHERE id = $2 AND user_id = $3"
    )
    .bind(&name)
    .bind(id)
    .bind(user.id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "Key not found"}))));
    }

    Ok(Json(json!({"message": "Key renamed", "name": name})))
}

pub async fn delete_key(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query(
        "DELETE FROM api_keys WHERE id = $1 AND user_id = $2"
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "Key not found"}))));
    }

    Ok(Json(json!({"message": "API key deleted"})))
}
