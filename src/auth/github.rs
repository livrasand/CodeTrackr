use axum::{
    extract::{Query, State},
    response::{Redirect, Json, Response},
    http::{StatusCode, header},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{AppState, auth::{create_jwt, generate_api_key, hash_api_key_with_secret}, error_handling};

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

    // Upsert user
    let user = sqlx::query_as::<_, crate::models::User>(
        r#"
        INSERT INTO users (id, username, display_name, email, avatar_url, github_id, country, timezone, plan, is_public, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, 'UTC', 'free', true, NOW(), NOW())
        ON CONFLICT (github_id) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            avatar_url = EXCLUDED.avatar_url,
            updated_at = NOW()
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&gh_user.login)
    .bind(&gh_user.name)
    .bind(&gh_user.email)
    .bind(&gh_user.avatar_url)
    .bind(&github_id)
    .bind(&gh_user.location)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| error_handling::handle_auth_error(e))?;

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

    let jwt = create_jwt(&user.id.to_string(), &state.config.jwt_secret)
        .map_err(|e| error_handling::handle_auth_error(e))?;

    // Generar código de un solo uso para canje seguro
    let exchange_code = Uuid::new_v4().to_string();
    let exchange_key = format!("auth_exchange:{}", exchange_code);
    
    // Almacenar JWT en Redis por 5 minutos (300 segundos)
    let exchange_stored = match state.redis.get_conn().await {
        Ok(mut conn) => {
            let result: Result<(), redis::RedisError> = redis::cmd("SETEX")
                .arg(&exchange_key)
                .arg(300) // 5 minutos TTL
                .arg(&jwt)
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
        codes.insert(exchange_code.clone(), jwt.clone());
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
    
    // Obtener JWT de Redis, con fallback a memoria
    let jwt = match state.redis.get_conn().await {
        Ok(mut conn) => {
            let token: Option<String> = match redis::cmd("GET")
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

            if token.is_some() {
                // Eliminar el código después de usarlo (single-use)
                let _: Result<(), _> = redis::cmd("DEL")
                    .arg(&exchange_key)
                    .query_async(&mut conn)
                    .await;
            } else {
                tracing::warn!("Exchange code not found in Redis, checking in-memory fallback");
            }

            token
        }
        Err(e) => {
            tracing::warn!("Redis unavailable during exchange_code ({}), checking in-memory fallback", e);
            None
        }
    };

    // Fallback: check in-memory store if Redis returned nothing
    let jwt = if jwt.is_none() {
        let mut codes = state.exchange_codes.lock().await;
        codes.remove(&payload.code)
    } else {
        jwt
    };

    let jwt = jwt.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid or expired exchange code"})))
    })?;

    // Crear cookie segura con el JWT
    let cookie_value = format!(
        "jwt={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={}",
        jwt,
        60 * 60 * 24 * 30 // 30 días en segundos
    );

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::SET_COOKIE, cookie_value)
        .header(header::CONTENT_TYPE, "application/json")
        .body(json!({"success": true, "token": jwt}).to_string().into())
        .map_err(|e| error_handling::handle_auth_error(e))?;

    Ok(response)
}
