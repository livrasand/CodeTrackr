use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

// ── User ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub github_id: Option<String>,
    pub gitlab_id: Option<String>,
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
    pub country: Option<String>,
    pub timezone: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

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

#[derive(Debug, Serialize)]
pub struct ApiKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ApiKeyCreated {
    pub id: Uuid,
    pub name: String,
    pub key: String,  // Only returned once
    pub created_at: DateTime<Utc>,
}

// ── Heartbeat ───────────────────────────────────────────────────────────────

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
    pub time: Option<f64>,  // Unix timestamp (WakaTime compat)
}

// ── Stats ────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct DailyStat {
    pub date: chrono::NaiveDate,
    pub total_seconds: i64,
    pub top_language: Option<String>,
    pub top_project: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LanguageStat {
    pub language: String,
    pub seconds: i64,
    pub percentage: f64,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectStat {
    pub project: String,
    pub seconds: i64,
    pub last_heartbeat: DateTime<Utc>,
    pub languages: Vec<String>,
}

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

// ── Claims (JWT) ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,     // user_id
    pub iat: i64,
    pub exp: i64,
}
