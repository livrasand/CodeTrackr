use axum::{
    extract::{Query, State},
    response::{Redirect, Json},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{AppState, auth::{create_access_token, generate_api_key, hash_api_key_with_secret}, models::CreateRefreshTokenRequest, error_handling};
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
    let base_url = if state.config.base_url.starts_with("http") {
        state.config.base_url.clone()
    } else {
        format!("https://{}", state.config.base_url)
    };
    
    let callback_url = format!("{}/auth/gitlab/callback", base_url);
    let encoded_redirect = urlencoding::encode(&callback_url);
    
    // Request more standard scopes to ensure profile access
    let scope = "read_user openid profile email";
    let encoded_scope = urlencoding::encode(scope);

    let url = format!(
        "https://gitlab.com/oauth/authorize?client_id={}&response_type=code&scope={}&redirect_uri={}",
        state.config.gitlab_client_id,
        encoded_scope,
        encoded_redirect
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

    let base_url = if state.config.base_url.starts_with("http") {
        state.config.base_url.clone()
    } else {
        format!("https://{}", state.config.base_url)
    };
    
    let callback_url = format!("{}/auth/gitlab/callback", base_url);

    let client = reqwest::Client::new();
    let token_res: GitlabTokenResponse = client
        .post("https://gitlab.com/oauth/token")
        .form(&[
            ("client_id", state.config.gitlab_client_id.as_str()),
            ("client_secret", state.config.gitlab_client_secret.as_str()),
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &callback_url),
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

    // 1. Try to find user by gitlab_id
    let existing_user = sqlx::query_as::<_, crate::models::User>(
        "SELECT * FROM users WHERE gitlab_id = $1"
    )
    .bind(&gitlab_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_auth_error(e))?;

    let user = if let Some(mut user) = existing_user {
        // Update existing user
        user = sqlx::query_as::<_, crate::models::User>(
            r#"
            UPDATE users SET
                display_name = COALESCE($1, display_name),
                avatar_url = COALESCE($2, avatar_url),
                updated_at = NOW()
            WHERE id = $3
            RETURNING *
            "#,
        )
        .bind(&gl_user.name)
        .bind(&gl_user.avatar_url)
        .bind(user.id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(|e| error_handling::handle_auth_error(e))?;
        user
    } else {
        // 2. User doesn't exist by gitlab_id, check if username is taken, try to link by email
        let mut final_username = gl_user.username.clone();

        // Intento de vinculación por email si existe
        let linked_user = if let Some(ref email) = gl_user.email {
            sqlx::query_as::<_, crate::models::User>(
                "UPDATE users SET gitlab_id = $1, updated_at = NOW() WHERE email = $2 AND gitlab_id IS NULL RETURNING *"
            )
            .bind(&gitlab_id)
            .bind(email)
            .fetch_optional(&state.db.pool)
            .await
            .unwrap_or(None)
        } else {
            None
        };

        if let Some(user) = linked_user {
            user
        } else {
            let username_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
                .bind(&final_username)
                .fetch_one(&state.db.pool)
                .await
                .map_err(|e| error_handling::handle_auth_error(e))?;

            if username_exists {
                // Username collision — append random suffix
                final_username = format!("{}-gl-{}", final_username, &Uuid::new_v4().to_string()[..4]);
            }

            // 3. Insert new user
            let mut insert_res = sqlx::query_as::<_, crate::models::User>(
                r#"
                INSERT INTO users (id, username, display_name, email, avatar_url, gitlab_id, country, timezone, plan, is_public, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, 'UTC', 'free', true, NOW(), NOW())
                RETURNING *
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(&final_username)
            .bind(&gl_user.name)
            .bind(&gl_user.email)
            .bind(&gl_user.avatar_url)
            .bind(&gitlab_id)
            .bind(&gl_user.location)
            .fetch_one(&state.db.pool)
            .await;

            // Si falla por duplicado de username, reintentar con más entropía
            if let Err(ref e) = insert_res {
                if let Some(db_err) = e.as_database_error() {
                    if db_err.constraint() == Some("users_username_key") {
                        tracing::info!("GitLab Username collision for {}, retrying...", final_username);
                        final_username = format!("{}-gl-{}", gl_user.username, &Uuid::new_v4().to_string()[..8]);
                        insert_res = sqlx::query_as::<_, crate::models::User>(
                            r#"
                            INSERT INTO users (id, username, display_name, email, avatar_url, gitlab_id, country, timezone, plan, is_public, created_at, updated_at)
                            VALUES ($1, $2, $3, $4, $5, $6, $7, 'UTC', 'free', true, NOW(), NOW())
                            RETURNING *
                            "#,
                        )
                        .bind(Uuid::new_v4())
                        .bind(&final_username)
                        .bind(&gl_user.name)
                        .bind(&gl_user.email)
                        .bind(&gl_user.avatar_url)
                        .bind(&gitlab_id)
                        .bind(&gl_user.location)
                        .fetch_one(&state.db.pool)
                        .await;
                    }
                }
            }

            insert_res.map_err(|e| error_handling::handle_auth_error(e))?
        }
    };

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

    let access_token = create_access_token(&user.id.to_string(), &state.config.jwt_secret)
        .map_err(|e| error_handling::handle_auth_error(e))?;

    // Crear refresh token con el nuevo sistema de rotación
    let refresh_request = CreateRefreshTokenRequest {
        device_id: format!("gitlab-{}", gl_user.username),
        device_info: Some(json!({
            "type": "gitlab_oauth",
            "gitlab_id": gl_user.id,
            "gitlab_username": gl_user.username,
            "created_at": chrono::Utc::now().to_rfc3339()
        })),
    };
    
    let refresh_response = crate::services::refresh_tokens::RefreshTokenService::create_token(
        user.id,
        refresh_request,
        None, // IP address no disponible en este contexto
        None, // User agent no disponible en este contexto
        &state,
    ).await.map_err(|e| {
        tracing::error!("Refresh token creation failed: {}", e);
        error_handling::handle_auth_error(e)
    })?;

    // Generar código de canje único para mayor seguridad
    let exchange_code = Uuid::new_v4().to_string();
    let token_data = json!({
        "access_token": access_token,
        "refresh_token": refresh_response.refresh_token
    });

    // Intentar almacenar en Redis primero
    let exchange_stored = match state.redis.get_conn().await {
        Ok(mut conn) => {
            let result: Result<(), _> = redis::cmd("SETEX")
                .arg(format!("auth_exchange:{}", exchange_code))
                .arg(600) // 10 minutos
                .arg(token_data.to_string())
                .query_async(&mut conn)
                .await;
            result.is_ok()
        }
        Err(e) => {
            tracing::error!("Redis get_conn failed during gitlab_callback: {}", e);
            false
        }
    };

    if !exchange_stored {
        // Redis unavailable — fall back to in-memory store
        tracing::warn!("Redis unavailable, storing exchange code in memory");
        let mut codes = state.exchange_codes.lock().await;
        codes.insert(exchange_code.clone(), (token_data, std::time::Instant::now()));
    }

    // Ejecutar hooks de lifecycle para 'on_user_login'
    let event_data = json!({
        "user_id": user.id,
        "username": user.username,
        "display_name": user.display_name,
        "provider": "gitlab",
        "is_new_user": user.created_at == user.updated_at // Simplificación para detectar si es nuevo usuario
    });
    crate::api::plugin_rpc::execute_lifecycle_hooks(&user.id, "on_user_login", event_data, &state).await;

    // Redirigir con código de canje en lugar del JWT (hash fragment para evitar interceptación por query-param handlers)
    Ok(Redirect::temporary(&format!(
        "{}/dashboard#exchange={}",
        state.config.frontend_url, exchange_code
    )))
}
