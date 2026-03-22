/// Refresh Token Service with Rotation
///
/// Implements secure refresh token rotation:
/// - Each refresh token is single-use (rotated on each use)
/// - Tokens are bound to devices/sessions
/// - Automatic cleanup of expired tokens
/// - Security monitoring and suspicious activity detection

use uuid::Uuid;
use chrono::{Utc, Duration};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;

use crate::{models::{RefreshToken, CreateRefreshTokenRequest, RefreshTokenResponse}, AppState};

type HmacSha256 = Hmac<Sha256>;

/// Refresh token service
pub struct RefreshTokenService;

impl RefreshTokenService {
    /// Hashes a refresh token for storage (public method)
    pub fn hash_token(token: &str, secret: &str) -> Result<String, String> {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|e| format!("HMAC error: {}", e))?;
        mac.update(token.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
    
    /// Creates a new refresh token with device binding
    pub async fn create_token(
        user_id: Uuid,
        request: CreateRefreshTokenRequest,
        ip_address: Option<String>,
        user_agent: Option<String>,
        state: &AppState,
    ) -> Result<RefreshTokenResponse, String> {
        // Generate raw token
        let raw_token = Self::generate_raw_token();
        let token_hash = Self::hash_token(&raw_token, &state.config.jwt_secret)?;
        
        // Calculate expiration (30 days from now)
        let expires_at = Utc::now() + Duration::days(30);
        
        // Store in database
        let refresh_token = sqlx::query_as::<_, RefreshToken>(
            r#"
            INSERT INTO refresh_tokens (
                user_id, token_hash, device_id, device_info, ip_address, user_agent,
                is_active, created_at, last_used_at, expires_at, usage_count, suspicious_activity
            ) VALUES (
                $1, $2, $3, $4, $5::inet, $6, true, NOW(), NOW(), $7, 0, false
            ) RETURNING *
            "#
        )
        .bind(user_id)
        .bind(&token_hash)
        .bind(&request.device_id)
        .bind(&request.device_info)
        .bind(ip_address)
        .bind(user_agent)
        .bind(expires_at)
        .fetch_one(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
        
        Ok(RefreshTokenResponse {
            refresh_token: raw_token,
            device_id: refresh_token.device_id,
            expires_at: refresh_token.expires_at,
        })
    }
    
    /// Rotates a refresh token (invalidates old, creates new)
    pub async fn rotate_token(
        raw_token: &str,
        ip_address: Option<String>,
        user_agent: Option<String>,
        state: &AppState,
    ) -> Result<RefreshTokenResponse, String> {
        let token_hash = Self::hash_token(raw_token, &state.config.jwt_secret)?;
        
        // Find and validate existing token
        let existing_token = sqlx::query_as::<_, RefreshToken>(
            "SELECT * FROM refresh_tokens WHERE token_hash = $1 AND is_active = true"
        )
        .bind(&token_hash)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("Invalid refresh token")?;
        
        // Check expiration
        if existing_token.expires_at < Utc::now() {
            return Err("Refresh token expired".to_string());
        }
        
        // Security checks
        if Self::detect_suspicious_activity(&existing_token, &ip_address, &user_agent) {
            // Mark as suspicious and revoke
            sqlx::query(
                "UPDATE refresh_tokens SET suspicious_activity = true, is_active = false WHERE id = $1"
            )
            .bind(existing_token.id)
            .execute(&state.db.pool)
            .await
            .map_err(|e| format!("Database error: {}", e))?;
            
            return Err("Suspicious activity detected".to_string());
        }
        
        // Update usage statistics
        sqlx::query(
            r#"
            UPDATE refresh_tokens 
            SET usage_count = usage_count + 1, last_used_at = NOW(), 
                ip_address = COALESCE($2::inet, ip_address), user_agent = COALESCE($3, user_agent)
            WHERE id = $1
            "#
        )
        .bind(existing_token.id)
        .bind(&ip_address)
        .bind(&user_agent)
        .execute(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
        
        // Create new token (rotation)
        let new_raw_token = Self::generate_raw_token();
        let new_token_hash = Self::hash_token(&new_raw_token, &state.config.jwt_secret)?;
        let new_expires_at = Utc::now() + Duration::days(30);
        
        // Insert new token
        let new_token = sqlx::query_as::<_, RefreshToken>(
            r#"
            INSERT INTO refresh_tokens (
                user_id, token_hash, device_id, device_info, ip_address, user_agent,
                is_active, created_at, last_used_at, expires_at, usage_count, suspicious_activity
            ) VALUES (
                $1, $2, $3, $4, $5::inet, $6, true, NOW(), NOW(), $7, 0, false
            ) RETURNING *
            "#
        )
        .bind(existing_token.user_id)
        .bind(&new_token_hash)
        .bind(&existing_token.device_id)
        .bind(&existing_token.device_info)
        .bind(&ip_address)
        .bind(&user_agent)
        .bind(new_expires_at)
        .fetch_one(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
        
        // Mark old token as rotated
        sqlx::query(
            "UPDATE refresh_tokens SET is_active = false, rotated_at = NOW() WHERE id = $1"
        )
        .bind(existing_token.id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
        
        Ok(RefreshTokenResponse {
            refresh_token: new_raw_token,
            device_id: new_token.device_id,
            expires_at: new_token.expires_at,
        })
    }
    
    /// Revokes a specific refresh token
    #[allow(dead_code)]
    pub async fn revoke_token(
        raw_token: &str,
        state: &AppState,
    ) -> Result<(), String> {
        let token_hash = Self::hash_token(raw_token, &state.config.jwt_secret)?;
        
        sqlx::query(
            "UPDATE refresh_tokens SET is_active = false, rotated_at = NOW() WHERE token_hash = $1"
        )
        .bind(&token_hash)
        .execute(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
        
        Ok(())
    }
    
    /// Revokes all refresh tokens for a user (e.g., password change)
    #[allow(dead_code)]
    pub async fn revoke_user_tokens(
        user_id: Uuid,
        state: &AppState,
    ) -> Result<i64, String> {
        let result = sqlx::query(
            "UPDATE refresh_tokens SET is_active = false, rotated_at = NOW() WHERE user_id = $1 AND is_active = true"
        )
        .bind(user_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
        
        Ok(result.rows_affected() as i64)
    }
    
    /// Lists active refresh tokens for a user
    pub async fn list_user_tokens(
        user_id: Uuid,
        state: &AppState,
    ) -> Result<Vec<RefreshToken>, String> {
        let tokens = sqlx::query_as::<_, RefreshToken>(
            "SELECT * FROM refresh_tokens WHERE user_id = $1 AND is_active = true ORDER BY last_used_at DESC"
        )
        .bind(user_id)
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| format!("Database error: {}", e))?;
        
        Ok(tokens)
    }
    
    /// Cleans up expired and inactive tokens
    pub async fn cleanup_expired_tokens(state: &AppState) -> Result<i64, String> {
        let result: Result<Option<i32>, sqlx::Error> = sqlx::query_scalar(
            "SELECT cleanup_expired_refresh_tokens()"
        )
        .fetch_one(&state.db.pool)
        .await;

        match result {
            Ok(count) => Ok(count.unwrap_or(0) as i64),
            Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("42883") => {
                // Fallback for older schemas without the function.
                tracing::warn!(
                    "cleanup_expired_refresh_tokens() missing; falling back to inline cleanup. \
                     Ensure migrations are up to date."
                );
                let res = sqlx::query(
                    "DELETE FROM refresh_tokens \
                     WHERE (expires_at < NOW() OR is_active = false) \
                     AND created_at < NOW() - INTERVAL '30 days'"
                )
                .execute(&state.db.pool)
                .await
                .map_err(|e| format!("Database error: {}", e))?;
                Ok(res.rows_affected() as i64)
            }
            Err(e) => Err(format!("Database error: {}", e)),
        }
    }
    
    /// Generates a cryptographically secure raw refresh token
    fn generate_raw_token() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
        format!("rt_{}", hex::encode(random_bytes))
    }
    
    /// Detects suspicious activity on refresh token usage
    fn detect_suspicious_activity(
        token: &RefreshToken,
        new_ip: &Option<String>,
        new_user_agent: &Option<String>,
    ) -> bool {
        // Check for IP address change
        if let (Some(token_ip), Some(new_ip_addr)) = (&token.ip_address, new_ip) {
            if token_ip != new_ip_addr {
                tracing::warn!(
                    "Suspicious activity: IP address changed for token {} from {} to {}",
                    token.id, token_ip, new_ip_addr
                );
                return true;
            }
        }
        
        // Check for user agent change
        if let (Some(token_ua), Some(new_ua)) = (&token.user_agent, new_user_agent) {
            if token_ua != new_ua {
                tracing::warn!(
                    "Suspicious activity: User agent changed for token {} from {} to {}",
                    token.id, token_ua, new_ua
                );
                return true;
            }
        }
        
        // Check for unusual usage patterns
        if token.usage_count > 1000 {
            tracing::warn!(
                "Suspicious activity: High usage count for token {}: {}",
                token.id, token.usage_count
            );
            return true;
        }
        
        false
    }
}
