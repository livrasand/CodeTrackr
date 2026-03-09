use axum::{
    extract::{State, Path, Query},
    response::Json,
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use chrono::Datelike;
use crate::AppState;

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub country: Option<String>,
    pub available_for_hire: Option<bool>,
}

pub async fn get_global(
    State(state): State<AppState>,
    Query(q): Query<LeaderboardQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let limit = q.limit.unwrap_or(100).min(500);
    let offset = q.offset.unwrap_or(0);
    let week_key = format!("lb:week:{}", chrono::Utc::now().format("%Y-W%W"));

    let mut entries: Vec<(String, f64)> = match state.redis.get_conn().await {
        Ok(mut conn) => redis::cmd("ZREVRANGEBYSCORE")
            .arg(&week_key)
            .arg("+inf")
            .arg("-inf")
            .arg("WITHSCORES")
            .arg("LIMIT")
            .arg(offset)
            .arg(limit)
            .query_async(&mut conn)
            .await
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    // Fallback to DB when Redis has no data for this week
    if entries.is_empty() {
        let now = chrono::Utc::now();
        let week_start = now - chrono::Duration::days(now.weekday().num_days_from_monday() as i64);
        let week_start = week_start.date_naive().and_hms_opt(0, 0, 0).unwrap();
        let rows = sqlx::query(
            r#"SELECT user_id::text, COALESCE(SUM(duration_seconds), 0)::float8 AS seconds
               FROM heartbeats
               WHERE recorded_at >= $1
               GROUP BY user_id
               ORDER BY seconds DESC
               LIMIT $2 OFFSET $3"#
        )
        .bind(week_start)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db.pool)
        .await
        .unwrap_or_default();

        use sqlx::Row;
        entries = rows.iter()
            .map(|r| (r.get::<String, _>("user_id"), r.get::<f64, _>("seconds")))
            .collect();
    }

    let mut leaderboard = Vec::new();
    for (i, (user_id_str, score)) in entries.iter().enumerate() {
        if let Ok(uid) = user_id_str.parse::<Uuid>() {
            let row = sqlx::query(
                "SELECT id, username, display_name, avatar_url, country, available_for_hire FROM users WHERE id = $1 AND is_public = true"
            )
            .bind(uid)
            .fetch_optional(&state.db.pool)
            .await
            .ok()
            .flatten();

            if let Some(user) = row {
                use sqlx::Row;
                let user_country: Option<String> = user.get("country");
                if let Some(ref country_filter) = q.country {
                    if user_country.as_deref() != Some(country_filter.as_str()) {
                        continue;
                    }
                }
                let user_available_for_hire: bool = user.get("available_for_hire");
                if q.available_for_hire == Some(true) && !user_available_for_hire {
                    continue;
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
                    "user_id": user.get::<Uuid, _>("id"),
                    "username": user.get::<String, _>("username"),
                    "display_name": user.get::<Option<String>, _>("display_name"),
                    "avatar_url": user.get::<Option<String>, _>("avatar_url"),
                    "country": user_country,
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

    let entries: Vec<(String, f64)> = match state.redis.get_conn().await {
        Ok(mut conn) => redis::cmd("ZREVRANGEBYSCORE")
            .arg(&week_key)
            .arg("+inf")
            .arg("-inf")
            .arg("WITHSCORES")
            .arg("LIMIT")
            .arg(offset)
            .arg(limit)
            .query_async(&mut conn)
            .await
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    let mut leaderboard = Vec::new();
    for (i, (user_id_str, score)) in entries.iter().enumerate() {
        if let Ok(uid) = user_id_str.parse::<Uuid>() {
            let row = sqlx::query(
                "SELECT id, username, display_name, avatar_url, country FROM users WHERE id = $1 AND is_public = true"
            )
            .bind(uid)
            .fetch_optional(&state.db.pool)
            .await
            .ok()
            .flatten();

            if let Some(user) = row {
                use sqlx::Row;
                let user_country: Option<String> = user.get("country");
                if let Some(ref country_filter) = q.country {
                    if user_country.as_deref() != Some(country_filter.as_str()) {
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
                    "username": user.get::<String, _>("username"),
                    "display_name": user.get::<Option<String>, _>("display_name"),
                    "avatar_url": user.get::<Option<String>, _>("avatar_url"),
                    "country": user_country,
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
            available_for_hire: q.available_for_hire,
        }),
    ).await
}
