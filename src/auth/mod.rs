pub mod github;
pub mod gitlab;
pub mod anonymous;

use axum::{
    extract::{Path, State, ConnectInfo},
    http::{StatusCode, header},
    response::{Json, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::{AppState, error_handling, services::refresh_tokens::RefreshTokenService};
use sqlx::Row;

pub async fn logout(
    State(state): State<AppState>,
    axum_extra::TypedHeader(auth): axum_extra::TypedHeader<axum_extra::headers::Authorization<axum_extra::headers::authorization::Bearer>>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let token = auth.token();
    if let Ok(claims) = verify_jwt(token, &state.config.jwt_secret) {
        let now = chrono::Utc::now().timestamp();
        let ttl = (claims.exp - now).max(1);
        
        // Blacklist por jti en lugar del token completo
        let key = format!("jwt_blacklist:{}", claims.jti);
        if let Ok(mut conn) = state.redis.get_conn().await {
            let _: Result<(), _> = redis::cmd("SETEX")
                .arg(&key)
                .arg(ttl)
                .arg("1")
                .query_async(&mut conn)
                .await;
        }
    }
    
    // Eliminar cookie JWT
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Set-Cookie", "jwt=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0")
        .header("Content-Type", "application/json")
        .body(json!({"message": "Logged out successfully"}).to_string().into())
        .map_err(|e| error_handling::handle_auth_error(e))?;
    
    Ok(response)
}

/// Lista todos los refresh tokens activos del usuario
pub async fn list_refresh_tokens(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let tokens = RefreshTokenService::list_user_tokens(user.id, &state)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list refresh tokens: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to list tokens"})))
        })?;
    
    let tokens_json: Vec<serde_json::Value> = tokens.into_iter().map(|t| {
        json!({
            "id": t.id,
            "device_id": t.device_id,
            "device_info": t.device_info,
            "ip_address": t.ip_address,
            "user_agent": t.user_agent,
            "created_at": t.created_at,
            "last_used_at": t.last_used_at,
            "expires_at": t.expires_at,
            "usage_count": t.usage_count,
            "suspicious_activity": t.suspicious_activity
        })
    }).collect();
    
    Ok(Json(json!({
        "tokens": tokens_json,
        "count": tokens_json.len()
    })))
}

/// Revoca un refresh token específico
pub async fn revoke_refresh_token(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(token_id): Path<uuid::Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Verificar que el token pertenece al usuario
    let _token = sqlx::query(
        "SELECT token_hash FROM refresh_tokens WHERE id = $1 AND user_id = $2 AND is_active = true"
    )
    .bind(token_id)
    .bind(user.id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("Database error: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Database error"})))
    })?
    .ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(json!({"error": "Token not found"})))
    })?;
    
    // Revocar el token
    sqlx::query(
        "UPDATE refresh_tokens SET is_active = false, rotated_at = NOW() WHERE id = $1"
    )
    .bind(token_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to revoke token: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to revoke token"})))
    })?;
    
    Ok(Json(json!({"message": "Token revoked successfully"})))
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
    #[allow(dead_code)]
    pub device_id: String,
}

#[derive(Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
}

pub async fn refresh_token(
    State(state): State<AppState>,
    ConnectInfo(conn): ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<RefreshRequest>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    // Extract client information for security monitoring
    let ip_address = Some(conn.ip().to_string());
    let user_agent = headers.get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    
    // Rotate refresh token with security checks
    let token_response = RefreshTokenService::rotate_token(
        &payload.refresh_token,
        ip_address,
        user_agent,
        &state,
    ).await.map_err(|e| {
        tracing::warn!("Refresh token rotation failed: {}", e);
        (StatusCode::UNAUTHORIZED, Json(json!({"error": e})))
    })?;
    
    // Get user_id from the rotated token
    let token_hash = RefreshTokenService::hash_token(&payload.refresh_token, &state.config.jwt_secret)
        .map_err(|e| {
            tracing::error!("Token hashing error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Authentication failed"})))
        })?;
    
    let old_token_row = sqlx::query(
        "SELECT user_id FROM refresh_tokens WHERE token_hash = $1"
    )
    .bind(&token_hash)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| {
        tracing::error!("Database error: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Authentication failed"})))
    })?
    .ok_or_else(|| {
        (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid token"})))
    })?;
    let old_token_user_id: uuid::Uuid = old_token_row.try_get("user_id")
        .map_err(|e| {
            tracing::error!("Failed to get user_id from token row: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Authentication failed"})))
        })?;
    
    // Generate new access token
    let new_access_token = create_access_token(&old_token_user_id.to_string(), &state.config.jwt_secret)
        .map_err(|e| {
            tracing::error!("Access token creation failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Authentication failed"})))
        })?;
    
    let response = Json(json!(RefreshResponse {
        access_token: new_access_token,
        refresh_token: token_response.refresh_token,
    }));
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(response.to_string().into())
        .map_err(|e| error_handling::handle_auth_error(e))?)
}

// ── JWT ──────────────────────────────────────────────────────────────────────

use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use crate::models::Claims;
use chrono::Utc;

pub fn create_jwt(user_id: &str, secret: &str, token_type: crate::models::TokenType, expires_in_seconds: i64) -> anyhow::Result<String> {
    use uuid::Uuid;
    
    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: user_id.to_string(),
        iat: now,
        exp: now + expires_in_seconds,
        jti: Uuid::new_v4().to_string(),
        token_type,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

// Función de compatibilidad temporal para access tokens de 15 minutos
pub fn create_access_token(user_id: &str, secret: &str) -> anyhow::Result<String> {
    create_jwt(user_id, secret, crate::models::TokenType::Access, 15 * 60) // 15 minutos
}

// Función para refresh tokens de 7 días
#[allow(dead_code)]
pub fn create_refresh_token(user_id: &str, secret: &str) -> anyhow::Result<String> {
    create_jwt(user_id, secret, crate::models::TokenType::Refresh, 7 * 24 * 60 * 60) // 7 días
}

pub fn verify_jwt(token: &str, secret: &str) -> anyhow::Result<Claims> {
    let validation = Validation::new(Algorithm::HS256);
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(token_data.claims)
}

// ── Auth extractor ────────────────────────────────────────────────────────────

use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap},
};

pub struct AuthenticatedUser(pub crate::models::User);

#[async_trait]
impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token(&parts.headers)
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "Missing or invalid Authorization header"})),
                )
            })?;

        // Try JWT first
        if let Ok(claims) = verify_jwt(&token, &state.config.jwt_secret) {
            // Validar que sea un access token (los refresh tokens no deben usarse para autenticación)
            if !matches!(claims.token_type, crate::models::TokenType::Access) {
                return Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid token type for authentication"}))));
            }

            // Check JWT blacklist by jti (populated on logout)
            let blacklist_key = format!("jwt_blacklist:{}", claims.jti);
            let is_blacklisted = async {
                let mut conn = state.redis.get_conn().await.ok()?;
                let val: Option<String> = redis::cmd("GET")
                    .arg(&blacklist_key)
                    .query_async(&mut conn)
                    .await
                    .ok()?;
                val
            }.await.is_some();

            if is_blacklisted {
                return Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "Token has been revoked"}))));
            }

            let user_id: uuid::Uuid = claims.sub.parse().map_err(|_| {
                (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid token"})))
            })?;

            let user = sqlx::query_as::<_, crate::models::User>(
                "SELECT * FROM users WHERE id = $1"
            )
            .bind(user_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(|e| {
                tracing::error!("JWT auth DB error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Authentication failed"})))
            })?
            .ok_or_else(|| {
                (StatusCode::UNAUTHORIZED, Json(json!({"error": "User not found"})))
            })?;

            return Ok(AuthenticatedUser(user));
        }

        // Try API key
        let key_hash = hash_api_key_with_secret(&token, &state.config.jwt_secret);
        let user = sqlx::query_as::<_, crate::models::User>(
            r#"
            SELECT u.* FROM users u
            INNER JOIN api_keys k ON k.user_id = u.id
            WHERE k.key_hash = $1
            "#,
        )
        .bind(&key_hash)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| {
            tracing::error!("API key auth DB error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Authentication failed"})))
        })?
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid API key"})))
        })?;

        // Update last_used_at
        let _ = sqlx::query(
            "UPDATE api_keys SET last_used_at = NOW() WHERE key_hash = $1"
        )
        .bind(&key_hash)
        .execute(&state.db.pool)
        .await;

        Ok(AuthenticatedUser(user))
    }
}

pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    // Bearer token
    if let Some(auth) = headers.get("Authorization") {
        let val = auth.to_str().ok()?;
        if let Some(tok) = val.strip_prefix("Bearer ") {
            return Some(tok.to_string());
        }
    }
    // X-API-Key header
    if let Some(key) = headers.get("X-API-Key") {
        return Some(key.to_str().ok()?.to_string());
    }
    // Cookie header (para JWT en cookie HttpOnly)
    if let Some(cookie) = headers.get("Cookie") {
        let val = cookie.to_str().ok()?;
        for pair in val.split(';') {
            let pair = pair.trim();
            if let Some(jwt) = pair.strip_prefix("jwt=") {
                return Some(jwt.to_string());
            }
        }
    }
    None
}

#[allow(dead_code)]
pub fn hash_api_key(_key: &str) -> String {
    // Esta función debe recibir el secret como parámetro para evitar inconsistencias
    panic!("hash_api_key debe ser llamada con jwt_secret como parámetro. Usa hash_api_key_with_secret(key, secret)");
}

pub fn hash_api_key_with_secret(key: &str, secret: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("JWT_SECRET debe tener al menos 32 bytes para HMAC-SHA256");
    mac.update(key.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn generate_api_key() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    format!("ct_{}", hex::encode(random_bytes))
}
