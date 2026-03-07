use axum::{
    extract::{State, Path},
    response::Json,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::{AppState, auth::AuthenticatedUser};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct StorePlugin {
    pub id: Uuid,
    pub author_id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub version: String,
    pub repository: Option<String>,
    pub icon: Option<String>,
    pub widget_type: Option<String>,
    pub api_endpoint: Option<String>,
    pub script: Option<String>,
    pub settings_schema: Option<Value>,
    pub is_published: bool,
    pub is_banned: bool,
    pub ban_reason: Option<String>,
    pub install_count: i32,
    pub avg_rating: f64,
    pub rating_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct PublishPluginRequest {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub repository: Option<String>,
    pub icon: Option<String>,
    pub widget_type: Option<String>,
    pub api_endpoint: Option<String>,
    pub script: Option<String>,
    pub settings_schema: Option<Value>,
}

#[derive(Deserialize)]
pub struct ReportPluginRequest {
    pub reason: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct BanPluginRequest {
    pub reason: Option<String>,
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

pub async fn list_store_plugins(
    State(state): State<AppState>,
) -> ApiResult {
    let rows = sqlx::query(
        r#"SELECT p.id, p.author_id, p.name, p.display_name, p.description, p.version,
                  p.repository, p.icon, p.widget_type, p.api_endpoint, p.script,
                  p.settings_schema, p.is_published, p.is_banned, p.ban_reason,
                  p.install_count,
                  COALESCE(p.avg_rating, 0.0)::float8 as avg_rating,
                  COALESCE(p.rating_count, 0) as rating_count,
                  p.created_at,
                  u.username as author_username
           FROM plugin_store p
           JOIN users u ON u.id = p.author_id
           WHERE p.is_published = true AND p.is_banned = false
           ORDER BY p.install_count DESC, p.created_at DESC"#
    )
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    let plugins: Vec<Value> = rows.iter().map(|r| {
        use sqlx::Row;
        json!({
            "id": r.get::<Uuid, _>("id"),
            "author_id": r.get::<Uuid, _>("author_id"),
            "author_username": r.get::<String, _>("author_username"),
            "name": r.get::<String, _>("name"),
            "display_name": r.get::<String, _>("display_name"),
            "description": r.get::<Option<String>, _>("description"),
            "version": r.get::<String, _>("version"),
            "repository": r.get::<Option<String>, _>("repository"),
            "icon": r.get::<Option<String>, _>("icon"),
            "script": r.get::<Option<String>, _>("script"),
            "install_count": r.get::<i32, _>("install_count"),
            "avg_rating": r.get::<f64, _>("avg_rating"),
            "rating_count": r.get::<i32, _>("rating_count"),
            "created_at": r.get::<DateTime<Utc>, _>("created_at"),
        })
    }).collect();

    Ok(Json(json!({ "plugins": plugins })))
}

pub async fn publish_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<PublishPluginRequest>,
) -> ApiResult {
    let version = body.version.unwrap_or_else(|| "0.1.0".to_string());
    let icon = body.icon.unwrap_or_else(|| "🔌".to_string());
    let widget_type = body.widget_type.unwrap_or_else(|| "counter".to_string());

    if let Some(ref desc) = body.description {
        if desc.chars().count() > 90 {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Description must be 90 characters or fewer"}))));
        }
    }

    let plugin = sqlx::query_as::<_, StorePlugin>(
        r#"INSERT INTO plugin_store
               (author_id, name, display_name, description, version, repository, icon, widget_type, api_endpoint, script, settings_schema)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
           ON CONFLICT (name) DO UPDATE SET
             display_name   = EXCLUDED.display_name,
             description    = EXCLUDED.description,
             version        = EXCLUDED.version,
             repository     = EXCLUDED.repository,
             icon           = EXCLUDED.icon,
             widget_type    = EXCLUDED.widget_type,
             api_endpoint   = EXCLUDED.api_endpoint,
             script         = EXCLUDED.script,
             settings_schema = EXCLUDED.settings_schema,
             updated_at     = NOW()
           RETURNING id, author_id, name, display_name, description, version, repository,
                     icon, widget_type, api_endpoint, script, settings_schema,
                     is_published, is_banned, ban_reason, install_count,
                     COALESCE(avg_rating, 0)::float8 AS avg_rating,
                     COALESCE(rating_count, 0) AS rating_count,
                     created_at"#
    )
    .bind(user.id)
    .bind(&body.name)
    .bind(&body.display_name)
    .bind(&body.description)
    .bind(&version)
    .bind(&body.repository)
    .bind(&icon)
    .bind(&widget_type)
    .bind(&body.api_endpoint)
    .bind(&body.script)
    .bind(&body.settings_schema)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "published", "plugin": plugin })))
}

pub async fn install_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    // Prevent installing banned plugins
    let banned: bool = sqlx::query_scalar(
        "SELECT is_banned FROM plugin_store WHERE id = $1"
    )
    .bind(plugin_id)
    .fetch_optional(&state.db.pool)
    .await
    .unwrap_or(None)
    .unwrap_or(true);

    if banned {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "This plugin is not available"}))));
    }

    sqlx::query(
        "INSERT INTO installed_plugins (user_id, plugin_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
    )
    .bind(user.id)
    .bind(plugin_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "installed" })))
}

pub async fn get_installed_plugins(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> ApiResult {
    let plugins = sqlx::query_as::<_, StorePlugin>(
        r#"SELECT p.id, p.author_id, p.name, p.display_name, p.description, p.version,
                  p.repository, p.icon, p.widget_type, p.api_endpoint, p.script,
                  p.settings_schema, p.is_published, p.is_banned, p.ban_reason,
                  p.install_count,
                  COALESCE(p.avg_rating, 0.0)::float8 as avg_rating,
                  COALESCE(p.rating_count, 0) as rating_count,
                  p.created_at
           FROM plugin_store p
           JOIN installed_plugins i ON p.id = i.plugin_id
           WHERE i.user_id = $1 AND p.is_banned = false"#
    )
    .bind(user.id)
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    Ok(Json(json!({ "installed": plugins })))
}

pub async fn uninstall_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    sqlx::query(
        "DELETE FROM installed_plugins WHERE user_id = $1 AND plugin_id = $2"
    )
    .bind(user.id)
    .bind(plugin_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "uninstalled" })))
}

// ── Reports ───────────────────────────────────────────────────────────────────

pub async fn report_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
    axum::Json(body): axum::Json<ReportPluginRequest>,
) -> ApiResult {
    let valid_reasons = ["spam", "malware", "inappropriate", "broken", "other"];
    if !valid_reasons.contains(&body.reason.as_str()) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid reason"}))));
    }

    sqlx::query(
        r#"INSERT INTO plugin_reports (plugin_id, reporter_id, reason, description)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (plugin_id, reporter_id) DO UPDATE SET
             reason = EXCLUDED.reason,
             description = EXCLUDED.description,
             resolved = false"#
    )
    .bind(plugin_id)
    .bind(user.id)
    .bind(&body.reason)
    .bind(&body.description)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "reported" })))
}

// ── Admin endpoints ───────────────────────────────────────────────────────────

pub async fn admin_ban_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
    axum::Json(body): axum::Json<BanPluginRequest>,
) -> ApiResult {
    if !user.is_admin {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Admin only"}))));
    }

    sqlx::query(
        "UPDATE plugin_store SET is_banned = true, ban_reason = $2 WHERE id = $1"
    )
    .bind(plugin_id)
    .bind(&body.reason)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "banned" })))
}

pub async fn admin_unban_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    if !user.is_admin {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Admin only"}))));
    }

    sqlx::query(
        "UPDATE plugin_store SET is_banned = false, ban_reason = NULL WHERE id = $1"
    )
    .bind(plugin_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "unbanned" })))
}

pub async fn author_delete_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    let rows = sqlx::query("DELETE FROM plugin_store WHERE id = $1 AND author_id = $2")
        .bind(plugin_id)
        .bind(user.id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    if rows.rows_affected() == 0 {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Not found or not your plugin"}))));
    }

    Ok(Json(json!({ "status": "deleted" })))
}

pub async fn admin_delete_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    if !user.is_admin {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Admin only"}))));
    }

    sqlx::query("DELETE FROM plugin_store WHERE id = $1")
        .bind(plugin_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "deleted" })))
}

pub async fn admin_list_plugins(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> ApiResult {
    if !user.is_admin {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Admin only"}))));
    }

    let plugins = sqlx::query_as::<_, StorePlugin>(
        r#"SELECT id, author_id, name, display_name, description, version, repository, icon,
                  widget_type, api_endpoint, script, settings_schema, is_published, is_banned,
                  ban_reason, install_count,
                  COALESCE(avg_rating, 0.0)::float8 as avg_rating,
                  COALESCE(rating_count, 0) as rating_count,
                  created_at
           FROM plugin_store ORDER BY created_at DESC"#
    )
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    Ok(Json(json!({ "plugins": plugins })))
}

pub async fn admin_list_reports(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> ApiResult {
    if !user.is_admin {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Admin only"}))));
    }

    let reports = sqlx::query(
        r#"SELECT r.id, r.reason, r.description, r.resolved, r.created_at,
                  p.id as plugin_id, p.display_name as plugin_name, p.name as plugin_slug,
                  u.username as reporter_username
           FROM plugin_reports r
           JOIN plugin_store p ON p.id = r.plugin_id
           JOIN users u ON u.id = r.reporter_id
           ORDER BY r.created_at DESC"#
    )
    .fetch_all(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let result: Vec<Value> = reports.iter().map(|r| {
        use sqlx::Row;
        json!({
            "id": r.get::<Uuid, _>("id"),
            "reason": r.get::<String, _>("reason"),
            "description": r.get::<Option<String>, _>("description"),
            "resolved": r.get::<bool, _>("resolved"),
            "created_at": r.get::<DateTime<Utc>, _>("created_at"),
            "plugin_id": r.get::<Uuid, _>("plugin_id"),
            "plugin_name": r.get::<String, _>("plugin_name"),
            "plugin_slug": r.get::<String, _>("plugin_slug"),
            "reporter_username": r.get::<String, _>("reporter_username"),
        })
    }).collect();

    Ok(Json(json!({ "reports": result, "count": result.len() })))
}

pub async fn get_plugin_script(
    AuthenticatedUser(_user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    let row = sqlx::query_as::<_, StorePlugin>(
        r#"SELECT id, author_id, name, display_name, description, version, repository, icon,
                  widget_type, api_endpoint, script, settings_schema, is_published, is_banned,
                  ban_reason, install_count,
                  COALESCE(avg_rating, 0.0)::float8 as avg_rating,
                  COALESCE(rating_count, 0) as rating_count,
                  created_at
           FROM plugin_store WHERE id = $1 AND is_banned = false AND is_published = true"#
    )
    .bind(plugin_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    match row {
        Some(p) => Ok(Json(json!({ "script": p.script }))),
        None => Err((StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found"})))),
    }
}

pub async fn get_plugin_accepted_version(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT accepted_version, accepted_script FROM installed_plugins WHERE user_id = $1 AND plugin_id = $2"
    )
    .bind(user.id)
    .bind(plugin_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    match row {
        Some((version, script)) => Ok(Json(json!({ "accepted_version": version, "accepted_script": script }))),
        None => Err((StatusCode::NOT_FOUND, Json(json!({"error": "Not installed"})))),
    }
}

pub async fn accept_plugin_version(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    // Fetch current version and script from plugin_store
    let plugin = sqlx::query_as::<_, StorePlugin>(
        r#"SELECT id, author_id, name, display_name, description, version, repository, icon,
                  widget_type, api_endpoint, script, settings_schema, is_published, is_banned,
                  ban_reason, install_count,
                  COALESCE(avg_rating, 0.0)::float8 as avg_rating,
                  COALESCE(rating_count, 0) as rating_count,
                  created_at
           FROM plugin_store WHERE id = $1 AND is_banned = false AND is_published = true"#
    )
    .bind(plugin_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Plugin not found"}))))?;

    sqlx::query(
        "UPDATE installed_plugins SET accepted_version = $1, accepted_script = $2 WHERE user_id = $3 AND plugin_id = $4"
    )
    .bind(&plugin.version)
    .bind(&plugin.script)
    .bind(user.id)
    .bind(plugin_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "accepted", "version": plugin.version })))
}

// ── Ratings & Reviews ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RatePluginRequest {
    pub rating: i16,
}

#[derive(Deserialize)]
pub struct ReviewPluginRequest {
    pub body: String,
}

#[derive(Deserialize)]
pub struct ScreenshotRequest {
    pub url: String,
    pub caption: Option<String>,
}

pub async fn rate_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
    axum::Json(body): axum::Json<RatePluginRequest>,
) -> ApiResult {
    if !(1..=5).contains(&body.rating) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Rating must be 1-5"}))));
    }
    sqlx::query(
        r#"INSERT INTO plugin_ratings (user_id, plugin_id, rating)
           VALUES ($1, $2, $3)
           ON CONFLICT (user_id, plugin_id) DO UPDATE SET rating = $3, created_at = NOW()"#
    )
    .bind(user.id).bind(plugin_id).bind(body.rating)
    .execute(&state.db.pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "status": "rated" })))
}

pub async fn review_plugin(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
    axum::Json(body): axum::Json<ReviewPluginRequest>,
) -> ApiResult {
    if body.body.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Review body is empty"}))));
    }
    sqlx::query(
        r#"INSERT INTO plugin_reviews (user_id, plugin_id, body)
           VALUES ($1, $2, $3)
           ON CONFLICT (user_id, plugin_id) DO UPDATE SET body = $3, created_at = NOW()"#
    )
    .bind(user.id).bind(plugin_id).bind(body.body.trim())
    .execute(&state.db.pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "status": "reviewed" })))
}

pub async fn get_plugin_detail(
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
) -> ApiResult {
    let plugin = sqlx::query_as::<_, StorePlugin>(
        r#"SELECT id, author_id, name, display_name, description, version, repository, icon,
                  widget_type, api_endpoint, script, settings_schema, is_published, is_banned,
                  ban_reason, install_count,
                  COALESCE(avg_rating, 0.0)::float8 as avg_rating,
                  COALESCE(rating_count, 0) as rating_count,
                  created_at
           FROM plugin_store WHERE id = $1 AND is_banned = false"#
    )
    .bind(plugin_id)
    .fetch_optional(&state.db.pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "Not found"}))))?;

    let reviews: Vec<Value> = sqlx::query(
        r#"SELECT r.body, r.created_at, u.username,
                  COALESCE(rt.rating, 0) as rating
           FROM plugin_reviews r
           JOIN users u ON u.id = r.user_id
           LEFT JOIN plugin_ratings rt ON rt.user_id = r.user_id AND rt.plugin_id = r.plugin_id
           WHERE r.plugin_id = $1
           ORDER BY r.created_at DESC
           LIMIT 50"#
    )
    .bind(plugin_id)
    .fetch_all(&state.db.pool).await
    .unwrap_or_default()
    .iter()
    .map(|r| {
        use sqlx::Row;
        json!({
            "username": r.get::<String, _>("username"),
            "body": r.get::<String, _>("body"),
            "rating": r.get::<i32, _>("rating"),
            "created_at": r.get::<DateTime<Utc>, _>("created_at"),
        })
    })
    .collect();

    let screenshots: Vec<Value> = sqlx::query(
        r#"SELECT s.url, s.caption, s.created_at, u.username
           FROM plugin_screenshots s
           JOIN users u ON u.id = s.user_id
           WHERE s.plugin_id = $1
           ORDER BY s.created_at DESC
           LIMIT 20"#
    )
    .bind(plugin_id)
    .fetch_all(&state.db.pool).await
    .unwrap_or_default()
    .iter()
    .map(|r| {
        use sqlx::Row;
        json!({
            "url": r.get::<String, _>("url"),
            "caption": r.get::<Option<String>, _>("caption"),
            "username": r.get::<String, _>("username"),
            "created_at": r.get::<DateTime<Utc>, _>("created_at"),
        })
    })
    .collect();

    let author_username: Option<String> = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1"
    ).bind(plugin.author_id).fetch_optional(&state.db.pool).await.unwrap_or(None);

    Ok(Json(json!({
        "plugin": plugin,
        "author_username": author_username,
        "reviews": reviews,
        "screenshots": screenshots,
    })))
}

pub async fn add_screenshot(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(plugin_id): Path<Uuid>,
    axum::Json(body): axum::Json<ScreenshotRequest>,
) -> ApiResult {
    let url_trimmed = body.url.trim();
    if url_trimmed.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "URL is required"}))));
    }
    if !url_trimmed.starts_with("http://") && !url_trimmed.starts_with("https://") {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Screenshot URL must start with http:// or https://"}))));
    }
    sqlx::query(
        "INSERT INTO plugin_screenshots (plugin_id, user_id, url, caption) VALUES ($1, $2, $3, $4)"
    )
    .bind(plugin_id).bind(user.id).bind(body.url.trim()).bind(&body.caption)
    .execute(&state.db.pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "status": "added" })))
}

pub async fn admin_resolve_report(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(report_id): Path<Uuid>,
) -> ApiResult {
    if !user.is_admin {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Admin only"}))));
    }

    sqlx::query("UPDATE plugin_reports SET resolved = true WHERE id = $1")
        .bind(report_id)
        .execute(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    Ok(Json(json!({ "status": "resolved" })))
}

