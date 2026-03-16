use axum::{
    extract::{Query, State},
    response::{Redirect, Json},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{AppState, auth::{create_jwt, generate_api_key, hash_api_key_with_secret}, error_handling};
use super::github::AuthQuery;

#[derive(Deserialize, Debug)]
struct GitlabTokenResponse {
    access_token: Option<String>,
}

#[derive(Deserialize, Debug)]
struct GitlabUser {
    id: i64,
    username: String,
    name: Option<String>,
    email: Option<String>,
    avatar_url: Option<String>,
    location: Option<String>,
}

pub async fn gitlab_login(State(state): State<AppState>) -> Redirect {
    if state.config.gitlab_client_id.is_empty() {
        return Redirect::temporary("/login?error=gitlab_not_configured");
    }
    let url = format!(
        "https://gitlab.com/oauth/authorize?client_id={}&response_type=code&scope=read_user&redirect_uri={}/auth/gitlab/callback",
        state.config.gitlab_client_id,
        state.config.base_url
    );
    Redirect::temporary(&url)
}

pub async fn gitlab_callback(
    Query(params): Query<AuthQuery>,
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    if let Some(_err) = params.error {
        return Ok(Redirect::temporary(&format!(
            "{}/login?error=gitlab_auth_denied",
            state.config.frontend_url
        )));
    }

    let code = params.code.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing code"})))
    })?;

    let client = reqwest::Client::new();
    let token_res: GitlabTokenResponse = client
        .post("https://gitlab.com/oauth/token")
        .form(&[
            ("client_id", state.config.gitlab_client_id.as_str()),
            ("client_secret", state.config.gitlab_client_secret.as_str()),
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &format!("{}/auth/gitlab/callback", state.config.base_url)),
        ])
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    let access_token = token_res.access_token.ok_or_else(|| {
        (StatusCode::UNAUTHORIZED, Json(json!({"error": "GitLab OAuth failed"})))
    })?;

    let gl_user: GitlabUser = client
        .get("https://gitlab.com/api/v4/user")
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    let gitlab_id = gl_user.id.to_string();

    let user = sqlx::query_as::<_, crate::models::User>(
        r#"
        INSERT INTO users (id, username, display_name, email, avatar_url, gitlab_id, country, timezone, plan, is_public, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'UTC', 'free', true, NOW(), NOW())
        ON CONFLICT (gitlab_id) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            avatar_url = EXCLUDED.avatar_url,
            updated_at = NOW()
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&gl_user.username)
    .bind(&gl_user.name)
    .bind(&gl_user.email)
    .bind(&gl_user.avatar_url)
    .bind(&gitlab_id)
    .bind(&gl_user.location)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_auth_error(e))?;

    let key_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(&state.db.pool)
        .await
        .unwrap_or(0);

    if key_count == 0 {
        let key = generate_api_key();
        let key_hash = hash_api_key_with_secret(&key, &state.config.jwt_secret);
        let prefix = key[..12].to_string();
        let _ = sqlx::query(
            "INSERT INTO api_keys (id, user_id, name, key_hash, key_prefix, created_at) VALUES ($1, $2, $3, $4, $5, NOW())"
        )
        .bind(Uuid::new_v4())
        .bind(user.id)
        .bind("Default Key")
        .bind(&key_hash)
        .bind(&prefix)
        .execute(&state.db.pool)
        .await;
    }

    let jwt = create_jwt(&user.id.to_string(), &state.config.jwt_secret)
        .map_err(|e| error_handling::handle_auth_error(e))?;

    // Ejecutar hooks de lifecycle para 'on_user_login'
    let event_data = json!({
        "user_id": user.id,
        "username": user.username,
        "display_name": user.display_name,
        "provider": "gitlab",
        "is_new_user": user.created_at == user.updated_at // Simplificación para detectar si es nuevo usuario
    });
    crate::api::plugin_rpc::execute_lifecycle_hooks(&user.id, "on_user_login", event_data, &state).await;

    Ok(Redirect::temporary(&format!(
        "{}/dashboard#token={}",
        state.config.frontend_url, jwt
    )))
}
