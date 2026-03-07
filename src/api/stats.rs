#[allow(unused)]
use axum::{
    extract::{State, Query},
    response::Json,
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;
use chrono::{DateTime, Utc, Duration};
use uuid::Uuid;

use crate::{AppState, auth::AuthenticatedUser, models::*};

#[derive(Deserialize)]
pub struct StatsQuery {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub range: Option<String>, // "7d" | "30d" | "90d" | "all"
}

fn parse_range(q: &StatsQuery) -> (DateTime<Utc>, DateTime<Utc>) {
    let end = q.end.unwrap_or_else(Utc::now);
    let start = q.start.unwrap_or_else(|| {
        match q.range.as_deref() {
            Some("30d") => end - Duration::days(30),
            Some("90d") => end - Duration::days(90),
            Some("all")  => DateTime::from_timestamp(0, 0).unwrap_or(end),
            _ => end - Duration::days(7),
        }
    });
    (start, end)
}

pub async fn get_summary(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (start, end) = parse_range(&q);

    let total: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM heartbeats WHERE user_id = $1 AND recorded_at BETWEEN $2 AND $3"
    )
    .bind(user.id).bind(start).bind(end)
    .fetch_one(&state.db.pool).await.ok().flatten();

    let days = (end - start).num_days().max(1);
    let daily_avg = total.unwrap_or(0) / days;

    let top_lang: Option<String> = sqlx::query_scalar(
        r#"SELECT language FROM heartbeats WHERE user_id = $1 AND recorded_at BETWEEN $2 AND $3
           AND language IS NOT NULL GROUP BY language ORDER BY SUM(duration_seconds) DESC LIMIT 1"#
    )
    .bind(user.id).bind(start).bind(end)
    .fetch_optional(&state.db.pool).await.ok().flatten();

    let top_proj: Option<String> = sqlx::query_scalar(
        r#"SELECT project FROM heartbeats WHERE user_id = $1 AND recorded_at BETWEEN $2 AND $3
           GROUP BY project ORDER BY SUM(duration_seconds) DESC LIMIT 1"#
    )
    .bind(user.id).bind(start).bind(end)
    .fetch_optional(&state.db.pool).await.ok().flatten();

    // Streaks (consecutive days with activity)
    let streak = calculate_streak(&state, user.id).await;

    Ok(Json(json!({
        "total_seconds": total.unwrap_or(0),
        "daily_average": daily_avg,
        "streak_current": streak.0,
        "streak_longest": streak.1,
        "top_language": top_lang,
        "top_project": top_proj,
        "range_start": start,
        "range_end": end,
    })))
}

pub async fn get_languages(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (start, end) = parse_range(&q);

    #[derive(sqlx::FromRow)]
    struct LangRow { language: Option<String>, seconds: Option<i64> }

    let rows = sqlx::query_as::<_, LangRow>(
        r#"SELECT language, CAST(SUM(duration_seconds) AS BIGINT) as seconds FROM heartbeats
           WHERE user_id = $1 AND recorded_at BETWEEN $2 AND $3 AND language IS NOT NULL
           GROUP BY language ORDER BY seconds DESC"#,
    )
    .bind(user.id).bind(start).bind(end)
    .fetch_all(&state.db.pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let total: i64 = rows.iter().filter_map(|r| r.seconds).sum();

    let languages: Vec<LanguageStat> = rows.into_iter()
        .filter_map(|r| {
            Some(LanguageStat {
                language: r.language?,
                seconds: r.seconds.unwrap_or(0),
                percentage: if total > 0 { r.seconds.unwrap_or(0) as f64 / total as f64 * 100.0 } else { 0.0 },
            })
        })
        .collect();

    Ok(Json(json!({"languages": languages, "total_seconds": total})))
}

pub async fn get_projects(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (start, end) = parse_range(&q);

    #[derive(sqlx::FromRow)]
    struct ProjRow { project: String, seconds: Option<i64>, last_heartbeat: Option<DateTime<Utc>> }

    let rows = sqlx::query_as::<_, ProjRow>(
        r#"SELECT project, CAST(SUM(duration_seconds) AS BIGINT) as seconds, MAX(recorded_at) as last_heartbeat
           FROM heartbeats WHERE user_id = $1 AND recorded_at BETWEEN $2 AND $3
           GROUP BY project ORDER BY seconds DESC"#,
    )
    .bind(user.id).bind(start).bind(end)
    .fetch_all(&state.db.pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let projects: Vec<serde_json::Value> = rows.into_iter().map(|r| {
        json!({
            "project": r.project,
            "seconds": r.seconds.unwrap_or(0),
            "last_heartbeat": r.last_heartbeat,
        })
    }).collect();

    Ok(Json(json!({"projects": projects})))
}

pub async fn get_daily(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (start, end) = parse_range(&q);

    #[derive(sqlx::FromRow)]
    struct DayRow { date: Option<chrono::NaiveDate>, seconds: Option<i64> }

    let rows = sqlx::query_as::<_, DayRow>(
        r#"SELECT DATE(recorded_at) as date, CAST(SUM(duration_seconds) AS BIGINT) as seconds
           FROM heartbeats WHERE user_id = $1 AND recorded_at BETWEEN $2 AND $3
           GROUP BY date ORDER BY date ASC"#,
    )
    .bind(user.id).bind(start).bind(end)
    .fetch_all(&state.db.pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let daily: Vec<serde_json::Value> = rows.into_iter().map(|r| {
        json!({ "date": r.date, "seconds": r.seconds.unwrap_or(0) })
    }).collect();

    Ok(Json(json!({"daily": daily})))
}

pub async fn get_streaks(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let (current, longest) = calculate_streak(&state, user.id).await;
    Ok(Json(json!({"current_streak": current, "longest_streak": longest})))
}

async fn calculate_streak(state: &AppState, user_id: Uuid) -> (i32, i32) {
    #[derive(sqlx::FromRow)]
    struct DateRow { date: Option<chrono::NaiveDate> }
    let rows = sqlx::query_as::<_, DateRow>(
        "SELECT DISTINCT DATE(recorded_at) as date FROM heartbeats WHERE user_id = $1 ORDER BY date DESC",
    )
    .bind(user_id)
    .fetch_all(&state.db.pool).await
    .unwrap_or_default();

    let today = Utc::now().date_naive();
    let mut current = 0i32;
    let mut longest = 0i32;
    let mut streak = 0i32;
    let mut last: Option<chrono::NaiveDate> = None;

    for row in &rows {
        if let Some(date) = row.date {
            if let Some(prev) = last {
                if prev - date == chrono::Duration::days(1) {
                    streak += 1;
                } else {
                    longest = longest.max(streak);
                    streak = 1;
                }
            } else {
                // First entry
                if date == today || date == today - chrono::Duration::days(1) {
                    streak = 1;
                    current = 1;
                } else {
                    streak = 1;
                }
            }
            last = Some(date);
        }
    }
    longest = longest.max(streak);

    // current streak = streak if started from today
    if let Some(first) = rows.first().and_then(|r| r.date) {
        if first == today || first == today - chrono::Duration::days(1) {
            current = streak;
        }
    }

    (current, longest)
}

pub async fn get_public_summary(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let users_tracked: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE is_public = true"
    )
    .fetch_one(&state.db.pool).await.ok();

    let total_seconds: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(duration_seconds), 0) FROM heartbeats"
    )
    .fetch_one(&state.db.pool).await.ok().flatten();

    Ok(Json(json!({
        "users": users_tracked.unwrap_or(0),
        "total_seconds": total_seconds.unwrap_or(0),
    })))
}
