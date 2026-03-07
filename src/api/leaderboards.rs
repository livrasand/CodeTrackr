use axum::{
    extract::{State, Path, Query},
    response::Json,
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::AppState;

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub country: Option<String>,
}

pub async fn get_global(
    State(state): State<AppState>,
    Query(q): Query<LeaderboardQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let limit = q.limit.unwrap_or(100).min(500);
    let offset = q.offset.unwrap_or(0);
    let week_key = format!("lb:week:{}", chrono::Utc::now().format("%Y-W%W"));

    let entries: Vec<(String, f64)> = if let Ok(mut guard) = state.redis.get_conn().await {
        if let Some(conn) = guard.as_mut() {
            redis::cmd("ZREVRANGEBYSCORE")
                .arg(&week_key)
                .arg("+inf")
                .arg("-inf")
                .arg("WITHSCORES")
                .arg("LIMIT")
                .arg(offset)
                .arg(limit)
                .query_async(conn)
                .await
                .unwrap_or_default()
        } else { vec![] }
    } else { vec![] };

    let mut leaderboard = Vec::new();
    for (i, (user_id_str, score)) in entries.iter().enumerate() {
        if let Ok(uid) = user_id_str.parse::<Uuid>() {
            if let Ok(user) = sqlx::query_as::<_, crate::models::User>(
                "SELECT * FROM users WHERE id = $1 AND is_public = true"
            )
            .bind(uid)
            .fetch_one(&state.db.pool)
            .await {
                if let Some(ref country_filter) = q.country {
                    if user.country.as_deref() != Some(country_filter.as_str()) {
                        continue;
                    }
                }
                let top_language: Option<String> = sqlx::query_scalar(
                    r#"SELECT language FROM heartbeats WHERE user_id = $1 AND language IS NOT NULL GROUP BY language ORDER BY SUM(duration_seconds) DESC LIMIT 1"#
                )
                .bind(uid).fetch_optional(&state.db.pool).await.ok().flatten();

                let top_editor: Option<String> = sqlx::query_scalar(
                    r#"SELECT editor FROM heartbeats WHERE user_id = $1 AND editor IS NOT NULL GROUP BY editor ORDER BY SUM(duration_seconds) DESC LIMIT 1"#
                )
                .bind(uid).fetch_optional(&state.db.pool).await.ok().flatten();

                let top_os: Option<String> = sqlx::query_scalar(
                    r#"SELECT os FROM heartbeats WHERE user_id = $1 AND os IS NOT NULL GROUP BY os ORDER BY SUM(duration_seconds) DESC LIMIT 1"#
                )
                .bind(uid).fetch_optional(&state.db.pool).await.ok().flatten();

                leaderboard.push(json!({
                    "rank": offset + i as i64 + 1,
                    "user_id": user.id,
                    "username": user.username,
                    "display_name": user.display_name,
                    "avatar_url": user.avatar_url,
                    "country": user.country,
                    "seconds": *score as i64,
                    "top_language": top_language,
                    "top_editor": top_editor,
                    "top_os": top_os,
                }));
            }
        }
    }

    Ok(Json(json!({
        "leaderboard": leaderboard,
        "week": chrono::Utc::now().format("%Y-W%W").to_string(),
        "updated_at": chrono::Utc::now(),
    })))
}

pub async fn get_by_language(
    State(state): State<AppState>,
    Path(lang): Path<String>,
    Query(q): Query<LeaderboardQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let limit = q.limit.unwrap_or(100).min(500);
    let offset = q.offset.unwrap_or(0);
    let week_key = format!("lb:lang:{}:{}", lang.to_lowercase(), chrono::Utc::now().format("%Y-W%W"));

    let entries: Vec<(String, f64)> = if let Ok(mut guard) = state.redis.get_conn().await {
        if let Some(conn) = guard.as_mut() {
            redis::cmd("ZREVRANGEBYSCORE")
                .arg(&week_key)
                .arg("+inf")
                .arg("-inf")
                .arg("WITHSCORES")
                .arg("LIMIT")
                .arg(offset)
                .arg(limit)
                .query_async(conn)
                .await
                .unwrap_or_default()
        } else { vec![] }
    } else { vec![] };

    let mut leaderboard = Vec::new();
    for (i, (user_id_str, score)) in entries.iter().enumerate() {
        if let Ok(uid) = user_id_str.parse::<Uuid>() {
            if let Ok(user) = sqlx::query_as::<_, crate::models::User>(
                "SELECT * FROM users WHERE id = $1 AND is_public = true"
            )
            .bind(uid)
            .fetch_one(&state.db.pool)
            .await {
                if let Some(ref country_filter) = q.country {
                    if user.country.as_deref() != Some(country_filter.as_str()) {
                        continue;
                    }
                }
                let top_editor: Option<String> = sqlx::query_scalar(
                    r#"SELECT editor FROM heartbeats WHERE user_id = $1 AND editor IS NOT NULL GROUP BY editor ORDER BY SUM(duration_seconds) DESC LIMIT 1"#
                )
                .bind(uid).fetch_optional(&state.db.pool).await.ok().flatten();

                let top_os: Option<String> = sqlx::query_scalar(
                    r#"SELECT os FROM heartbeats WHERE user_id = $1 AND os IS NOT NULL GROUP BY os ORDER BY SUM(duration_seconds) DESC LIMIT 1"#
                )
                .bind(uid).fetch_optional(&state.db.pool).await.ok().flatten();

                leaderboard.push(json!({
                    "rank": offset + i as i64 + 1,
                    "username": user.username,
                    "display_name": user.display_name,
                    "avatar_url": user.avatar_url,
                    "country": user.country,
                    "seconds": *score as i64,
                    "language": lang,
                    "top_language": lang,
                    "top_editor": top_editor,
                    "top_os": top_os,
                }));
            }
        }
    }

    Ok(Json(json!({
        "language": lang,
        "leaderboard": leaderboard,
        "week": chrono::Utc::now().format("%Y-W%W").to_string(),
    })))
}

pub async fn get_by_country(
    State(state): State<AppState>,
    Path(country): Path<String>,
    Query(q): Query<LeaderboardQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Delegate to global with country filter
    get_global(
        State(state),
        Query(LeaderboardQuery {
            limit: q.limit,
            offset: q.offset,
            country: Some(country),
        }),
    ).await
}
