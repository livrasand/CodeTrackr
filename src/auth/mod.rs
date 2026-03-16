pub mod github;
pub mod gitlab;
pub mod anonymous;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Json, Response},
};
use serde_json::json;
use crate::{AppState, error_handling};

pub async fn logout(
    State(state): State<AppState>,
    axum_extra::TypedHeader(auth): axum_extra::TypedHeader<axum_extra::headers::Authorization<axum_extra::headers::authorization::Bearer>>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let token = auth.token();
    if let Ok(claims) = verify_jwt(token, &state.config.jwt_secret) {
        let now = chrono::Utc::now().timestamp();
        let ttl = (claims.exp - now).max(1);
        let key = format!("jwt_blacklist:{}", token);
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

// ── JWT ──────────────────────────────────────────────────────────────────────

use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use crate::models::Claims;
use chrono::Utc;

pub fn create_jwt(user_id: &str, secret: &str) -> anyhow::Result<String> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: user_id.to_string(),
        iat: now,
        exp: now + 60 * 60 * 24 * 30, // 30 days
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
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
            // Fix #10: Check JWT blacklist (populated on logout)
            let blacklist_key = format!("jwt_blacklist:{}", token);
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
