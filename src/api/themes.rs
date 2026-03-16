use axum::{
    extract::{State, Path},
    response::Json,
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{AppState, auth::AuthenticatedUser, error_handling};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

// ── Allowed CSS variable keys (whitelist to prevent injection) ────────────────
const ALLOWED_VARS: &[&str] = &[
    "--bg", "--bg-card", "--bg-input", "--bg-hover",
    "--text-main", "--text-muted", "--text-dark",
    "--border", "--border-focus",
    "--radius-sm", "--radius", "--radius-pill",
    "--transition",
    "--accent",
];

fn sanitize_variables(vars: &Value) -> Value {
    let obj = match vars.as_object() {
        Some(o) => o,
        None => return json!({}),
    };
    let mut clean = serde_json::Map::new();
    for (k, v) in obj {
        if !ALLOWED_VARS.contains(&k.as_str()) {
            continue;
        }
        if let Some(val_str) = v.as_str() {
            // Reject values with semicolons, braces, or url() to prevent injection
            if val_str.contains(';') || val_str.contains('{') || val_str.contains('}')
                || val_str.to_lowercase().contains("url(")
                || val_str.to_lowercase().contains("expression(")
            {
                continue;
            }
            clean.insert(k.clone(), json!(val_str));
        }
    }
    Value::Object(clean)
}

fn sanitize_custom_css(_css: Option<&str>) -> Option<String> {
    // Security: Reject all custom CSS - only allow CSS variables
    // This prevents XSS via CSS injection (url(), expression(), @import, etc.)
    None
}

// ── Handlers ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PublishThemeRequest {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub icon: Option<String>,
    pub variables: Value,
    pub custom_css: Option<String>,
}

pub async fn list_themes(
    State(state): State<AppState>,
) -> ApiResult {
    let rows = sqlx::query(
        r#"SELECT t.id, t.author_id, t.name, t.display_name, t.description,
                  t.version, t.icon, t.variables, t.custom_css,
                  t.install_count, t.created_at,
                  u.username as author_username
           FROM theme_store t
           JOIN users u ON u.id = t.author_id
           WHERE t.is_published = true AND t.is_banned = false
           ORDER BY t.install_count DESC, t.created_at DESC"#,
    )
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    let themes: Vec<Value> = rows.iter().map(|r| {
        use sqlx::Row;
        json!({
            "id": r.get::<Uuid, _>("id"),
            "author_username": r.get::<String, _>("author_username"),
            "name": r.get::<String, _>("name"),
            "display_name": r.get::<String, _>("display_name"),
            "description": r.get::<Option<String>, _>("description"),
            "version": r.get::<String, _>("version"),
            "icon": r.get::<String, _>("icon"),
            "variables": r.get::<Value, _>("variables"),
            "custom_css": r.get::<Option<String>, _>("custom_css"),
            "install_count": r.get::<i32, _>("install_count"),
        })
    }).collect();

    Ok(Json(json!({ "themes": themes })))
}

pub async fn publish_theme(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<PublishThemeRequest>,
) -> ApiResult {
    let name = body.name.trim().to_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "name must be lowercase kebab-case"}))));
    }
    if body.display_name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "display_name is required"}))));
    }
    let clean_vars = sanitize_variables(&body.variables);
    let clean_css = sanitize_custom_css(body.custom_css.as_deref());
    let version = body.version.unwrap_or_else(|| "1.0.0".to_string());
    let icon = body.icon.unwrap_or_else(|| "🎨".to_string());

    sqlx::query(
        r#"INSERT INTO theme_store (author_id, name, display_name, description, version, icon, variables, custom_css)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
           ON CONFLICT (name) DO UPDATE SET
             display_name = EXCLUDED.display_name,
             description  = EXCLUDED.description,
             version      = EXCLUDED.version,
             icon         = EXCLUDED.icon,
             variables    = EXCLUDED.variables,
             custom_css   = EXCLUDED.custom_css,
             updated_at   = NOW()
           WHERE theme_store.author_id = $1"#,
    )
    .bind(user.id)
    .bind(&name)
    .bind(body.display_name.trim())
    .bind(body.description.as_deref().map(|s| s.trim()))
    .bind(&version)
    .bind(&icon)
    .bind(&clean_vars)
    .bind(&clean_css)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({ "status": "published" })))
}

pub async fn install_theme(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(theme_id): Path<Uuid>,
) -> ApiResult {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM theme_store WHERE id = $1 AND is_published = true AND is_banned = false)"
    )
    .bind(theme_id)
    .fetch_one(&state.db.pool)
    .await
    .unwrap_or(false);

    if !exists {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "Theme not found"}))));
    }

    sqlx::query(
        "INSERT INTO installed_themes (user_id, theme_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
    )
    .bind(user.id)
    .bind(theme_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    sqlx::query(
        "UPDATE theme_store SET install_count = install_count + 1 WHERE id = $1"
    )
    .bind(theme_id)
    .execute(&state.db.pool)
    .await
    .ok();

    Ok(Json(json!({ "status": "installed" })))
}

pub async fn uninstall_theme(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Path(theme_id): Path<Uuid>,
) -> ApiResult {
    sqlx::query(
        "DELETE FROM installed_themes WHERE user_id = $1 AND theme_id = $2"
    )
    .bind(user.id)
    .bind(theme_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    // If this was the active theme, clear it
    sqlx::query(
        "UPDATE user_theme_prefs SET active_theme_id = NULL, updated_at = NOW()
         WHERE user_id = $1 AND active_theme_id = $2"
    )
    .bind(user.id)
    .bind(theme_id)
    .execute(&state.db.pool)
    .await
    .ok();

    Ok(Json(json!({ "status": "uninstalled" })))
}

#[derive(Deserialize)]
pub struct ApplyThemeRequest {
    pub theme_id: Option<Uuid>,
    pub custom_vars: Option<Value>,
}

pub async fn apply_theme(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<ApplyThemeRequest>,
) -> ApiResult {
    let clean_vars = body.custom_vars
        .as_ref()
        .map(sanitize_variables)
        .unwrap_or_else(|| json!({}));

    sqlx::query(
        r#"INSERT INTO user_theme_prefs (user_id, active_theme_id, custom_vars)
           VALUES ($1, $2, $3)
           ON CONFLICT (user_id) DO UPDATE SET
             active_theme_id = EXCLUDED.active_theme_id,
             custom_vars     = EXCLUDED.custom_vars,
             updated_at      = NOW()"#,
    )
    .bind(user.id)
    .bind(body.theme_id)
    .bind(&clean_vars)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({ "status": "applied" })))
}

pub async fn get_active_theme(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> ApiResult {
    let prefs = sqlx::query(
        r#"SELECT p.active_theme_id, p.custom_vars,
                  t.name, t.display_name, t.variables, t.custom_css, t.icon
           FROM user_theme_prefs p
           LEFT JOIN theme_store t ON t.id = p.active_theme_id
           WHERE p.user_id = $1"#,
    )
    .bind(user.id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    match prefs {
        Some(row) => {
            use sqlx::Row;
            Ok(Json(json!({
                "active_theme_id": row.get::<Option<Uuid>, _>("active_theme_id"),
                "theme_name": row.get::<Option<String>, _>("name"),
                "theme_display_name": row.get::<Option<String>, _>("display_name"),
                "theme_icon": row.get::<Option<String>, _>("icon"),
                "variables": row.get::<Option<Value>, _>("variables").unwrap_or_else(|| json!({})),
                "custom_css": row.get::<Option<String>, _>("custom_css"),
                "custom_vars": row.get::<Value, _>("custom_vars"),
            })))
        }
        None => Ok(Json(json!({
            "active_theme_id": null,
            "variables": {},
            "custom_vars": {},
        }))),
    }
}

pub async fn get_installed_themes(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> ApiResult {
    let rows = sqlx::query(
        r#"SELECT t.id, t.name, t.display_name, t.description, t.version,
                  t.icon, t.variables, t.custom_css, t.install_count,
                  u.username as author_username
           FROM theme_store t
           JOIN installed_themes it ON t.id = it.theme_id
           JOIN users u ON u.id = t.author_id
           WHERE it.user_id = $1 AND t.is_banned = false"#,
    )
    .bind(user.id)
    .fetch_all(&state.db.pool)
    .await
    .unwrap_or_default();

    let themes: Vec<Value> = rows.iter().map(|r| {
        use sqlx::Row;
        json!({
            "id": r.get::<Uuid, _>("id"),
            "author_username": r.get::<String, _>("author_username"),
            "name": r.get::<String, _>("name"),
            "display_name": r.get::<String, _>("display_name"),
            "description": r.get::<Option<String>, _>("description"),
            "version": r.get::<String, _>("version"),
            "icon": r.get::<String, _>("icon"),
            "variables": r.get::<Value, _>("variables"),
            "custom_css": r.get::<Option<String>, _>("custom_css"),
            "install_count": r.get::<i32, _>("install_count"),
        })
    }).collect();

    Ok(Json(json!({ "themes": themes })))
}
