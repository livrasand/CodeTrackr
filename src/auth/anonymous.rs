use axum::{
    extract::State,
    http::{StatusCode, header},
    response::{Json, Response},
};
use serde_json::{json, Value};
use uuid::Uuid;
use rand::Rng;
use crate::{AppState, auth, models::User};

/// Generate a random 16-digit account number (Mullvad-style)
fn generate_account_number() -> String {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| rng.gen_range(0..10).to_string()).collect()
}

/// Create anonymous account - generates account number and returns JWT
pub async fn create_anonymous_account(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = generate_account_number();
    
    // Check for collision (extremely unlikely but we handle it)
    let existing = sqlx::query_scalar!(
        "SELECT id FROM users WHERE account_number = $1",
        account_number
    )
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error checking account number collision: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Database error"})),
        )
    })?;

    if existing.is_some() {
        // Extremely unlikely collision — regenerate and retry inline
        let account_number = generate_account_number();
        let existing2 = sqlx::query_scalar!(
            "SELECT id FROM users WHERE account_number = $1",
            account_number
        )
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| {
            tracing::error!("DB error checking account number collision (retry): {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;
        if existing2.is_some() {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Could not generate unique account number"})),
            ));
        }
        let _ = account_number; // will shadow below
    }

    // Create user with anonymous account
    let user_id = Uuid::new_v4();
    let username = format!("user-{}", account_number);
    
    let user = sqlx::query_as!(
        User,
        r#"
        INSERT INTO users (
            id, username, account_number, plan, is_public, 
            timezone, created_at, updated_at
        ) VALUES (
            $1, $2, $3, 'free', true, 'UTC', NOW(), NOW()
        ) RETURNING *
        "#,
        user_id,
        username,
        account_number
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to create anonymous user: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to create account"})),
        )
    })?;

    // Create JWT token
    let token = auth::create_jwt(&user_id.to_string(), &state.config.jwt_secret)
        .map_err(|e| {
            tracing::error!("JWT creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    let response = Json(json!({
        "token": token,
        "user": {
            "id": user.id,
            "username": user.username,
            "account_number": user.account_number,
            "plan": user.plan,
            "created_at": user.created_at
        },
        "message": "Account created successfully. Save your account number: you'll need it to login."
    }));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(response.to_string().into())
        .map_err(|e| crate::error_handling::handle_auth_error(e))?)
}

/// Login with account number - returns JWT if account exists
pub async fn login_with_account_number(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = payload.get("account_number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "account_number is required"})),
            )
        })?;

    // Validate account number format (16 digits)
    if account_number.len() != 16 || !account_number.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid account number format"})),
        ));
    }

    // Find user by account number
    let user = sqlx::query_as!(
        User,
        "SELECT * FROM users WHERE account_number = $1",
        account_number
    )
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error finding user by account number: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Database error"})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid account number"})),
        )
    })?;

    // Create JWT token
    let token = auth::create_jwt(&user.id.to_string(), &state.config.jwt_secret)
        .map_err(|e| {
            tracing::error!("JWT creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    let response = Json(json!({
        "token": token,
        "user": {
            "id": user.id,
            "username": user.username,
            "account_number": user.account_number,
            "plan": user.plan,
            "created_at": user.created_at
        }
    }));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(response.to_string().into())
        .map_err(|e| crate::error_handling::handle_auth_error(e))?)
}

/// Verify account number exists (for frontend validation)
pub async fn verify_account_number(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let account_number = payload.get("account_number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "account_number is required"})),
            )
        })?;

    // Validate format
    if account_number.len() != 16 || !account_number.chars().all(|c| c.is_ascii_digit()) {
        return Ok(Json(json!({
            "valid": false,
            "message": "Invalid account number format"
        })));
    }

    // Check if exists
    let exists = sqlx::query_scalar!(
        "SELECT EXISTS(SELECT 1 FROM users WHERE account_number = $1)",
        account_number
    )
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error verifying account number: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Database error"})),
        )
    })?;

    Ok(Json(json!({
        "valid": exists.unwrap_or(false),
        "message": if exists.unwrap_or(false) {
            "Account number found"
        } else {
            "Account number not found"
        }
    })))
}
