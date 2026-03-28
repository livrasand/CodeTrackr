//! Core domain models shared across the application.
//!
//! Each struct maps to a database table (via `sqlx::FromRow`) or represents a
//! serializable API payload. Sensitive fields (OAuth IDs, billing identifiers,
//! key hashes) are intentionally absent from public-facing types like
//! [`PublicUser`] and [`ApiKeyResponse`].

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

// ── User ────────────────────────────────────────────────────────────────────

/// Full user record as stored in the `users` table.
///
/// Contains all fields including sensitive ones (OAuth IDs, billing, key hashes).
/// Never serialize this directly to API responses — use [`PublicUser`] instead.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub github_id: Option<String>,
    pub gitlab_id: Option<String>,
    pub account_number: Option<String>,
    pub is_anonymous: bool,
    pub plan: String,         // "free" | "pro"
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub plan_expires_at: Option<DateTime<Utc>>,
    pub is_public: bool,
    pub is_admin: bool,
    pub bio: Option<String>,
    pub website: Option<String>,
    pub profile_show_languages: bool,
    pub profile_show_projects: bool,
    pub profile_show_activity: bool,
    pub profile_show_plugins: bool,
    pub profile_show_streak: bool,
    pub available_for_hire: bool,
    pub show_in_leaderboard: bool,
    pub country: Option<String>,
    pub timezone: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Publicly safe subset of [`User`] for API responses and leaderboards.
///
/// Strips all sensitive fields: email, OAuth IDs, billing identifiers, and flags.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct PublicUser {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub plan: String,
    pub country: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<User> for PublicUser {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            avatar_url: u.avatar_url,
            plan: u.plan,
            country: u.country,
            created_at: u.created_at,
        }
    }
}

// ── API Key ─────────────────────────────────────────────────────────────────

/// Full API key record from the `api_keys` table.
///
/// `key_hash` is a bcrypt/argon2 hash of the raw key and must never be exposed.
/// Use [`ApiKeyResponse`] for list endpoints and [`ApiKeyCreated`] for the
/// single creation response that returns the raw key.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,  // First 8 chars for display
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Safe API key representation for list/detail responses.
///
/// Omits `key_hash`; exposes only `key_prefix` (first 8 chars) for display.
#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Response returned once when a new API key is created.
///
/// `key` contains the full raw key and is only returned at creation time.
/// It is never stored in plaintext — the server only keeps `key_hash`.
#[derive(Debug, Serialize)]
pub struct ApiKeyCreated {
    pub id: Uuid,
    pub name: String,
    pub key: String,  // Only returned once
    pub created_at: DateTime<Utc>,
}

// ── Heartbeat ───────────────────────────────────────────────────────────────

/// A coding activity event recorded by the editor extension.
///
/// Heartbeats are the primary data source for all stats. Each heartbeat captures
/// a moment of coding activity: the project, file, language, editor, and
/// duration. They are immutable once stored.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Heartbeat {
    pub id: Uuid,
    pub user_id: Uuid,
    pub project: String,
    pub file: Option<String>,
    pub language: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub workspace_root: Option<String>,
    pub package_path: Option<String>,    // For monorepo awareness
    pub duration_seconds: i32,
    pub is_write: bool,
    pub editor: Option<String>,
    pub os: Option<String>,
    pub machine: Option<String>,
    pub recorded_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Inbound heartbeat payload from the editor extension.
///
/// Maps to the JSON body of `POST /api/v1/heartbeats`. `duration` defaults to 0
/// if omitted; `is_write` defaults to `false`. `time` is a Unix timestamp
/// (float) used to de-duplicate heartbeats submitted out of order.
#[derive(Debug, Deserialize)]
pub struct HeartbeatRequest {
    pub project: String,
    pub file: Option<String>,
    #[serde(rename = "lang")]
    pub language: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub workspace_root: Option<String>,
    pub package_path: Option<String>,
    pub duration: Option<i32>,
    pub is_write: Option<bool>,
    pub editor: Option<String>,
    pub os: Option<String>,
    pub machine: Option<String>,
    pub time: Option<f64>,  // Unix timestamp
}

// ── Stats ────────────────────────────────────────────────────────────────────

/// Aggregated coding stats for a single calendar day.
///
/// Populated from the `daily_stats_cache` table which is refreshed periodically
/// from raw heartbeats.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct DailyStat {
    pub date: chrono::NaiveDate,
    pub total_seconds: i64,
    pub top_language: Option<String>,
    pub top_project: Option<String>,
}

/// Coding time breakdown for a single programming language.
#[derive(Debug, Serialize, Deserialize)]
pub struct LanguageStat {
    pub language: String,
    pub seconds: i64,
    pub percentage: f64,
}

/// Coding time breakdown for a single project.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectStat {
    pub project: String,
    pub seconds: i64,
    pub last_heartbeat: DateTime<Utc>,
    pub languages: Vec<String>,
}

/// Aggregated stats summary for a date range.
///
/// Returned by the stats API and used to render the dashboard overview.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    pub total_seconds: i64,
    pub daily_average: i64,
    pub streak_current: i32,
    pub streak_longest: i32,
    pub top_language: Option<String>,
    pub top_project: Option<String>,
    pub languages: Vec<LanguageStat>,
    pub projects: Vec<ProjectStat>,
    pub range_start: DateTime<Utc>,
    pub range_end: DateTime<Utc>,
}

// ── Leaderboard ──────────────────────────────────────────────────────────────

/// A single entry in a public leaderboard.
///
/// `rank` is 1-indexed and computed by the leaderboard query. `seconds` is the
/// total coding time for the leaderboard period.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: i64,
    pub user_id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub seconds: i64,
    pub language: Option<String>,
    pub country: Option<String>,
}

// ── Refresh Token ─────────────────────────────────────────────────────────────

/// Refresh token record with rotation tracking.
///
/// Stores refresh tokens with device binding and security monitoring.
/// Tokens are rotated on each use for enhanced security.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub device_id: String,
    pub device_info: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub usage_count: i32,
    pub suspicious_activity: bool,
}

/// Request payload for refresh token creation.
#[derive(Debug, Deserialize)]
pub struct CreateRefreshTokenRequest {
    pub device_id: String,
    pub device_info: Option<serde_json::Value>,
}

/// Response when creating a new refresh token.
#[derive(Debug, Serialize)]
pub struct RefreshTokenResponse {
    pub refresh_token: String,  // Raw token (only returned once)
    pub device_id: String,
    pub expires_at: DateTime<Utc>,
}

// ── Claims (JWT) ─────────────────────────────────────────────────────────────

/// JWT claims payload for session tokens.
///
/// `sub` is the user's UUID as a string. `iat` and `exp` are Unix timestamps.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,     // user_id
    pub iat: i64,
    pub exp: i64,
    pub jti: String,     // JWT ID para revocación granular
    pub token_type: TokenType,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum TokenType {
    Access,
    Refresh,
}
