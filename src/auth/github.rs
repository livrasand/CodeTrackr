use axum::{
    extract::{Query, State},
    response::{Redirect, Json, Response},
    http::{StatusCode, header},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{AppState, auth::{create_access_token, generate_api_key, hash_api_key_with_secret}, models::CreateRefreshTokenRequest, error_handling};

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct AuthQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GithubTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize, Debug)]
struct GithubUser {
    id: i64,
    login: String,
    name: Option<String>,
    email: Option<String>,
    avatar_url: Option<String>,
    location: Option<String>,
}

pub async fn github_login(State(state): State<AppState>) -> Redirect {
    let url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&scope=read:user,user:email&state=codetrackr",
        state.config.github_client_id
    );
    Redirect::temporary(&url)
}

pub async fn github_callback(
    Query(params): Query<AuthQuery>,
    State(state): State<AppState>,
) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    if let Some(err) = params.error {
        tracing::warn!("GitHub OAuth error: {}", err);
        return Ok(Redirect::temporary(&format!(
            "{}/login?error=github_auth_denied",
            state.config.frontend_url
        )));
    }

    let code = params.code.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing code"})))
    })?;

    // Exchange code for token
    let client = reqwest::Client::new();
    let token_res: GithubTokenResponse = client
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", &state.config.github_client_id),
            ("client_secret", &state.config.github_client_secret),
            ("code", &code),
        ])
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    if let Some(ref gh_error) = token_res.error {
        tracing::error!("GitHub token exchange error: {}", gh_error);
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "GitHub OAuth failed", "reason": gh_error})),
        ));
    }

    let access_token = token_res.access_token.ok_or_else(|| {
        (StatusCode::UNAUTHORIZED, Json(json!({"error": "GitHub OAuth failed"})))
    })?;

    // Get user info
    let gh_user: GithubUser = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("token {}", access_token))
        .header("User-Agent", "CodeTrackr/1.0")
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?
        .json()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    let github_id = gh_user.id.to_string();

    // 1. Try to find user by github_id
    let existing_user = sqlx::query_as::<_, crate::models::User>(
        "SELECT * FROM users WHERE github_id = $1"
    )
    .bind(&github_id)
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
        .bind(&gh_user.name)
        .bind(&gh_user.avatar_url)
        .bind(user.id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(|e| error_handling::handle_auth_error(e))?;
        user
    } else {
        // 2. Check if username is taken, try to link by email if ID is missing
        let mut final_username = gh_user.login.clone();

        // Si tenemos email verificado, intentamos vincular por email antes de crear uno nuevo
        let linked_user = if let Some(ref email) = gh_user.email {
            sqlx::query_as::<_, crate::models::User>(
                "UPDATE users SET github_id = $1, updated_at = NOW() WHERE email = $2 AND github_id IS NULL RETURNING *"
            )
            .bind(&github_id)
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
                final_username = format!("{}-gh-{}", final_username, &Uuid::new_v4().to_string()[..4]);
            }

            // 3. Insert new user
            let mut insert_res = sqlx::query_as::<_, crate::models::User>(
                r#"
                INSERT INTO users (id, username, display_name, email, avatar_url, github_id, country, timezone, plan, is_public, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, 'UTC', 'free', true, NOW(), NOW())
                RETURNING *
                "#,
            )
            .bind(Uuid::new_v4())
            .bind(&final_username)
            .bind(&gh_user.name)
            .bind(&gh_user.email)
            .bind(&gh_user.avatar_url)
            .bind(&github_id)
            .bind(&gh_user.location)
            .fetch_one(&state.db.pool)
            .await;

            // Si falla por duplicado de username (race condition), reintentar con sufijo extra
            if let Err(ref e) = insert_res {
                if let Some(db_err) = e.as_database_error() {
                    if db_err.constraint() == Some("users_username_key") {
                        tracing::info!("Username collision for {}, retrying with more entropy...", final_username);
                        final_username = format!("{}-gh-{}", gh_user.login, &Uuid::new_v4().to_string()[..8]);
                        insert_res = sqlx::query_as::<_, crate::models::User>(
                            r#"
                            INSERT INTO users (id, username, display_name, email, avatar_url, github_id, country, timezone, plan, is_public, created_at, updated_at)
                            VALUES ($1, $2, $3, $4, $5, $6, $7, 'UTC', 'free', true, NOW(), NOW())
                            RETURNING *
                            "#,
                        )
                        .bind(Uuid::new_v4())
                        .bind(&final_username)
                        .bind(&gh_user.name)
                        .bind(&gh_user.email)
                        .bind(&gh_user.avatar_url)
                        .bind(&github_id)
                        .bind(&gh_user.location)
                        .fetch_one(&state.db.pool)
                        .await;
                    }
                }
            }

            insert_res.map_err(|e| error_handling::handle_auth_error(e))?
        }
    };

    // Create default API key if user has none
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
        device_id: format!("github-{}", gh_user.login),
        device_info: Some(json!({
            "type": "github_oauth",
            "github_id": gh_user.id,
            "github_login": gh_user.login,
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

    // Generar código de un solo uso para canje seguro
    let exchange_code = Uuid::new_v4().to_string();
    let exchange_key = format!("auth_exchange:{}", exchange_code);
    
    // Almacenar ambos tokens en Redis por 5 minutos (300 segundos)
    let token_data = json!({
        "access_token": access_token,
        "refresh_token": refresh_response.refresh_token
    });
    
    let exchange_stored = match state.redis.get_conn().await {
        Ok(mut conn) => {
            let result: Result<(), redis::RedisError> = redis::cmd("SETEX")
                .arg(&exchange_key)
                .arg(300) // 5 minutos TTL
                .arg(token_data.to_string())
                .query_async(&mut conn)
                .await;
            if let Err(ref e) = result {
                tracing::error!("Redis SETEX failed for exchange code {}: {}", exchange_code, e);
            }
            result.is_ok()
        }
        Err(e) => {
            tracing::error!("Redis get_conn failed during github_callback: {}", e);
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
        "provider": "github",
        "is_new_user": user.created_at == user.updated_at // Simplificación para detectar si es nuevo usuario
    });
    crate::api::plugin_rpc::execute_lifecycle_hooks(&user.id, "on_user_login", event_data, &state).await;

    // Redirigir con código de canje en lugar del JWT (hash fragment para evitar interceptación por query-param handlers)
    Ok(Redirect::temporary(&format!(
        "{}/dashboard#exchange={}",
        state.config.frontend_url, exchange_code
    )))
}

#[derive(Deserialize)]
pub struct ExchangeRequest {
    pub code: String,
}

pub async fn exchange_code(
    State(state): State<AppState>,
    Json(payload): Json<ExchangeRequest>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let exchange_key = format!("auth_exchange:{}", payload.code);
    
    // Obtener token data de Redis, con fallback a memoria
    let token_data_str = match state.redis.get_conn().await {
        Ok(mut conn) => {
            let data: Option<String> = match redis::cmd("GET")
                .arg(&exchange_key)
                .query_async(&mut conn)
                .await
            {
                Ok(val) => val,
                Err(e) => {
                    tracing::error!("Redis GET failed for exchange key {}: {}", exchange_key, e);
                    None
                }
            };

            if data.is_some() {
                // Eliminar el código después de usarlo (single-use)
                let _: Result<(), _> = redis::cmd("DEL")
                    .arg(&exchange_key)
                    .query_async(&mut conn)
                    .await;
            } else {
                tracing::warn!("Exchange code not found in Redis, checking in-memory fallback");
            }

            data
        }
        Err(e) => {
            tracing::warn!("Redis unavailable during exchange_code ({}), checking in-memory fallback", e);
            None
        }
    };

    let token_data: Option<serde_json::Value> = if token_data_str.is_none() {
        let mut codes = state.exchange_codes.lock().await;
        codes.remove(&payload.code).map(|(data, _)| data)
    } else {
        token_data_str.and_then(|s| serde_json::from_str(&s).ok())
    };

    let token_data = token_data.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid or expired exchange code"})))
    })?;

    let access_token = token_data.get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Missing access token"}))))?;

    let refresh_token = token_data.get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Missing refresh token"}))))?;

    // Crear cookie segura con el access token (tiempo reducido)
    let cookie_value = format!(
        "jwt={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={}",
        access_token,
        15 * 60 // 15 minutos en segundos
    );

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, cookie_value)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json!({
            "success": true, 
            "access_token": access_token,
            "refresh_token": refresh_token
        }).to_string().into())
        .map_err(|e| error_handling::handle_auth_error(e))?;

    Ok(response)
}
