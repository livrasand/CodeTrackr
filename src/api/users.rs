use axum::{
    extract::{State, Path},
    response::{Json, Response, IntoResponse},
    http::{StatusCode, header},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{AppState, auth::AuthenticatedUser, error_handling};

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct UpdateProfileRequest {
    pub bio: Option<String>,
    pub website: Option<String>,
    pub is_public: Option<bool>,
    pub profile_show_languages: Option<bool>,
    pub profile_show_projects: Option<bool>,
    pub profile_show_activity: Option<bool>,
    pub profile_show_plugins: Option<bool>,
    pub profile_show_streak: Option<bool>,
    pub available_for_hire: Option<bool>,
    pub show_in_leaderboard: Option<bool>,
}

pub async fn get_me(
    AuthenticatedUser(user): AuthenticatedUser,
    State(_state): State<AppState>,
) -> Json<serde_json::Value> {
    Json(json!({
        "id": user.id,
        "username": user.username,
        "display_name": user.display_name,
        "email": user.email,
        "avatar_url": user.avatar_url,
        "plan": user.plan,
        "is_public": user.is_public,
        "is_admin": user.is_admin,
        "bio": user.bio,
        "website": user.website,
        "profile_show_languages": user.profile_show_languages,
        "profile_show_projects": user.profile_show_projects,
        "profile_show_activity": user.profile_show_activity,
        "profile_show_plugins": user.profile_show_plugins,
        "profile_show_streak": user.profile_show_streak,
        "available_for_hire": user.available_for_hire,
        "show_in_leaderboard": user.show_in_leaderboard,
        "country": user.country,
        "timezone": user.timezone,
        "created_at": user.created_at,
    }))
}

pub async fn get_public_profile(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let user = sqlx::query_as::<_, crate::models::User>(
        "SELECT * FROM users WHERE username = $1 AND is_public = true"
    )
    .bind(&username)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "User not found"}))))?;

    // Follower / following counts
    let follower_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_follows WHERE following_id = $1"
    ).bind(user.id).fetch_one(&state.db.pool).await.unwrap_or(0);

    let following_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_follows WHERE follower_id = $1"
    ).bind(user.id).fetch_one(&state.db.pool).await.unwrap_or(0);

    // Stats (respecting profile visibility settings)
    let weekly_seconds: i64 = if user.profile_show_activity {
        sqlx::query_scalar(
            "SELECT COALESCE(SUM(duration_seconds), 0) FROM heartbeats WHERE user_id = $1 AND recorded_at > NOW() - INTERVAL '7 days'"
        ).bind(user.id).fetch_one(&state.db.pool).await.unwrap_or(0)
    } else { 0 };

    let streak: i64 = if user.profile_show_streak {
        sqlx::query_scalar(
            r#"WITH days AS (
                SELECT DISTINCT DATE(recorded_at) d FROM heartbeats WHERE user_id = $1
                ORDER BY d DESC
            ), numbered AS (
                SELECT d, ROW_NUMBER() OVER (ORDER BY d DESC) rn FROM days
            )
            SELECT COUNT(*) FROM numbered WHERE d = CURRENT_DATE - (rn - 1) * INTERVAL '1 day'"#
        ).bind(user.id).fetch_one(&state.db.pool).await.unwrap_or(0)
    } else { 0 };

    let languages: Vec<serde_json::Value> = if user.profile_show_languages {
        sqlx::query(
            r#"SELECT language, SUM(duration_seconds) as secs FROM heartbeats
               WHERE user_id = $1 AND language IS NOT NULL AND recorded_at > NOW() - INTERVAL '30 days'
               GROUP BY language ORDER BY secs DESC LIMIT 8"#
        ).bind(user.id).fetch_all(&state.db.pool).await.unwrap_or_default()
        .iter().map(|r| {
            use sqlx::Row;
            json!({ "language": r.get::<String,_>("language"), "seconds": r.get::<i64,_>("secs") })
        }).collect()
    } else { vec![] };

    let projects: Vec<serde_json::Value> = if user.profile_show_projects {
        sqlx::query(
            r#"SELECT project, SUM(duration_seconds) as secs FROM heartbeats
               WHERE user_id = $1 AND recorded_at > NOW() - INTERVAL '30 days'
               GROUP BY project ORDER BY secs DESC LIMIT 6"#
        ).bind(user.id).fetch_all(&state.db.pool).await.unwrap_or_default()
        .iter().map(|r| {
            use sqlx::Row;
            json!({ "project": r.get::<String,_>("project"), "seconds": r.get::<i64,_>("secs") })
        }).collect()
    } else { vec![] };

    let plugins: Vec<serde_json::Value> = if user.profile_show_plugins {
        sqlx::query(
            r#"SELECT id, name, display_name, description, version, icon,
                      install_count, COALESCE(avg_rating, 0.0)::float8 as avg_rating, rating_count
               FROM plugin_store WHERE author_id = $1 AND is_published = true AND is_banned = false
               ORDER BY install_count DESC LIMIT 10"#
        ).bind(user.id).fetch_all(&state.db.pool).await.unwrap_or_default()
        .iter().map(|r| {
            use sqlx::Row;
            json!({
                "id": r.get::<Uuid,_>("id"),
                "name": r.get::<String,_>("name"),
                "display_name": r.get::<String,_>("display_name"),
                "description": r.get::<Option<String>,_>("description"),
                "version": r.get::<String,_>("version"),
                "icon": r.get::<Option<String>,_>("icon"),
                "install_count": r.get::<i32,_>("install_count"),
                "avg_rating": r.get::<f64,_>("avg_rating"),
                "rating_count": r.get::<i32,_>("rating_count"),
            })
        }).collect()
    } else { vec![] };

    Ok(Json(json!({
        "username": user.username,
        "display_name": user.display_name,
        "avatar_url": user.avatar_url,
        "bio": user.bio,
        "website": user.website,
        "country": user.country,
        "plan": user.plan,
        "member_since": user.created_at,
        "follower_count": follower_count,
        "following_count": following_count,
        "show_languages": user.profile_show_languages,
        "show_projects": user.profile_show_projects,
        "show_activity": user.profile_show_activity,
        "show_plugins": user.profile_show_plugins,
        "show_streak": user.profile_show_streak,
        "available_for_hire": user.available_for_hire,
        "weekly_seconds": weekly_seconds,
        "streak_days": streak,
        "languages": languages,
        "projects": projects,
        "plugins": plugins,
    })))
}

pub async fn update_profile(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    axum::Json(body): axum::Json<UpdateProfileRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(ref url) = body.website {
        let url_trimmed = url.trim();
        if !url_trimmed.is_empty()
            && !url_trimmed.starts_with("http://")
            && !url_trimmed.starts_with("https://")
        {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "website must start with http:// or https://"}))));
        }
    }

    sqlx::query(
        r#"UPDATE users SET
            bio                    = COALESCE($1, bio),
            website                = COALESCE($2, website),
            is_public              = COALESCE($3, is_public),
            profile_show_languages = COALESCE($4, profile_show_languages),
            profile_show_projects  = COALESCE($5, profile_show_projects),
            profile_show_activity  = COALESCE($6, profile_show_activity),
            profile_show_plugins   = COALESCE($7, profile_show_plugins),
            profile_show_streak    = COALESCE($8, profile_show_streak),
            available_for_hire     = COALESCE($9, available_for_hire),
            show_in_leaderboard    = COALESCE($10, show_in_leaderboard),
            updated_at             = NOW()
           WHERE id = $11"#
    )
    .bind(&body.bio)
    .bind(&body.website)
    .bind(body.is_public)
    .bind(body.profile_show_languages)
    .bind(body.profile_show_projects)
    .bind(body.profile_show_activity)
    .bind(body.profile_show_plugins)
    .bind(body.profile_show_streak)
    .bind(body.available_for_hire)
    .bind(body.show_in_leaderboard)
    .bind(user.id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({ "status": "updated" })))
}

pub async fn follow_user(
    AuthenticatedUser(me): AuthenticatedUser,
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let target: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM users WHERE username = $1"
    ).bind(&username).fetch_optional(&state.db.pool).await
    .map_err(|e| error_handling::handle_database_error(e))?;

    let (target_id,) = target.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "User not found"}))))?;

    if target_id == me.id {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Cannot follow yourself"}))));
    }

    sqlx::query(
        "INSERT INTO user_follows (follower_id, following_id) VALUES ($1, $2) ON CONFLICT DO NOTHING"
    ).bind(me.id).bind(target_id).execute(&state.db.pool).await
    .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({ "status": "following" })))
}

pub async fn unfollow_user(
    AuthenticatedUser(me): AuthenticatedUser,
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let target: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM users WHERE username = $1"
    ).bind(&username).fetch_optional(&state.db.pool).await
    .map_err(|e| error_handling::handle_database_error(e))?;

    let (target_id,) = target.ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "User not found"}))))?;

    sqlx::query(
        "DELETE FROM user_follows WHERE follower_id = $1 AND following_id = $2"
    ).bind(me.id).bind(target_id).execute(&state.db.pool).await
    .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({ "status": "unfollowed" })))
}

pub async fn is_following(
    AuthenticatedUser(me): AuthenticatedUser,
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let target: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM users WHERE username = $1"
    ).bind(&username).fetch_optional(&state.db.pool).await
    .map_err(|e| error_handling::handle_database_error(e))?;

    let (target_id,) = match target {
        Some(t) => t,
        None => return Ok(Json(json!({ "following": false }))),
    };

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM user_follows WHERE follower_id = $1 AND following_id = $2)"
    ).bind(me.id).bind(target_id).fetch_one(&state.db.pool).await.unwrap_or(false);

    Ok(Json(json!({ "following": exists })))
}

/// SVG badge suitable for GitHub READMEs
pub async fn get_badge(
    State(state): State<AppState>,
    Path((username, lang)): Path<(String, String)>,
) -> Response {
    let total: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(h.duration_seconds), 0) FROM heartbeats h
           INNER JOIN users u ON u.id = h.user_id
           WHERE u.username = $1 AND h.language = $2
           AND h.recorded_at > NOW() - INTERVAL '7 days'"#
    )
    .bind(&username)
    .bind(&lang)
    .fetch_one(&state.db.pool)
    .await
    .unwrap_or(0);

    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let label = format!("{}h {}m this week", hours, minutes);
    let color = if hours >= 10 { "brightgreen" } else if hours >= 5 { "yellow" } else { "lightgrey" };

    let mut svg = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"220\" height=\"20\">");
    svg.push_str("<linearGradient id=\"s\" x2=\"0\" y2=\"100%\">");
    svg.push_str("<stop offset=\"0\" stop-color=\"#bbb\" stop-opacity=\".1\"/>");
    svg.push_str("<stop offset=\"1\" stop-opacity=\".1\"/></linearGradient>");
    svg.push_str("<rect rx=\"3\" width=\"220\" height=\"20\" fill=\"#555\"/>");
    svg.push_str(&format!("<rect rx=\"3\" x=\"110\" width=\"110\" height=\"20\" fill=\"{color}\"/>"));
    svg.push_str("<rect rx=\"3\" width=\"220\" height=\"20\" fill=\"url(#s)\"/>");
    svg.push_str("<g fill=\"#fff\" text-anchor=\"middle\" font-family=\"Verdana,Geneva,sans-serif\" font-size=\"11\">");
    svg.push_str(&format!("<text x=\"55\" y=\"15\" fill=\"#010101\" fill-opacity=\".3\">CodeTrackr {lang}</text>"));
    svg.push_str(&format!("<text x=\"55\" y=\"14\">CodeTrackr {lang}</text>"));
    svg.push_str(&format!("<text x=\"165\" y=\"15\" fill=\"#010101\" fill-opacity=\".3\">{label}</text>"));
    svg.push_str(&format!("<text x=\"165\" y=\"14\">{label}</text>"));
    svg.push_str("</g></svg>");

    (
        [(header::CONTENT_TYPE, "image/svg+xml"),
         (header::CACHE_CONTROL, "no-cache")],
        svg,
    ).into_response()
}

#[derive(Deserialize)]
pub struct ContactDevRequest {
    pub message: String,
}

pub async fn contact_dev(
    AuthenticatedUser(sender): AuthenticatedUser,
    State(state): State<AppState>,
    Path(username): Path<String>,
    axum::Json(body): axum::Json<ContactDevRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Verify target user exists, is public, and available for hire
    let user = sqlx::query_as::<_, crate::models::User>(
        "SELECT * FROM users WHERE username = $1 AND is_public = true AND available_for_hire = true"
    )
    .bind(&username)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "User not found or not available for hire"}))))?;

    if body.message.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "message is required"}))));
    }

    if body.message.len() > 2000 {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "message too long (max 2000 chars)"}))));
    }

    // Prevent self-contact
    if user.id == sender.id {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Cannot contact yourself"}))));
    }

    // Store the contact request in the database using authenticated user's data
    sqlx::query(
        r#"INSERT INTO hire_contacts (target_user_id, sender_name, sender_email, message)
           VALUES ($1, $2, $3, $4)"#
    )
    .bind(user.id)
    .bind(&sender.display_name)
    .bind(&sender.email)
    .bind(body.message.trim())
    .execute(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_database_error(e))?;

    Ok(Json(json!({ "status": "sent" })))
}
