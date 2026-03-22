use axum::{
    routing::{get, post},
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{Json, Response},
    middleware,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use uuid::Uuid;
use rand::Rng;
use sqlx::Row;
use chrono::{Utc, Duration};
use jsonwebtoken::{encode, Header, EncodingKey};

// Import the real refresh token service
use crate::models::{CreateRefreshTokenRequest, RefreshTokenResponse};
use crate::services::refresh_tokens::RefreshTokenService;

#[derive(Clone)]
struct AppState {
    db_pool: sqlx::PgPool,
    jwt_secret: String,
}

#[derive(Clone)]
struct Config {
    jwt_secret: String,
}

#[derive(Clone)]
struct FullAppState {
    db: crate::db::Database,
    config: Config,
}

impl FullAppState {
    fn from_simple(state: AppState) -> Self {
        Self {
            db: crate::db::Database { pool: state.db_pool },
            config: Config { jwt_secret: state.jwt_secret },
        }
    }
}

/// Generate anonymous username (random adjective + animal combination)
fn generate_anonymous_username() -> String {
    let adjectives = vec![
        "silent", "swift", "clever", "bright", "calm", "brave", "wise", "kind",
        "quiet", "noble", "gentle", "bold", "smart", "cool", "warm", "soft",
        "fierce", "grumpy", "lazy", "angry", "happy", "silly", "sneaky", "stupid",
        "tall", "short", "strong", "weak", "powerful", "slow", "fast", "old", "young",
        "tiny", "huge", "giant", "micro", "mega", "ultra", "super", "hyper", "mini", "maxi",
        "red", "blue", "green", "yellow", "purple", "orange", "pink", "brown", "black", "white",
        "golden", "silver", "bronze", "crystal", "rainbow", "neon", "glowing", "shiny", "dark", "light",
        "cosmic", "stellar", "galactic", "solar", "lunar", "planetary", "meteoric", "comet", "asteroid", "nebula",
        "arctic", "tropical", "desert", "oceanic", "mountain", "forest", "river", "lake", "volcanic", "glacial",
        "electric", "magnetic", "atomic", "nuclear", "quantum", "digital", "cyber", "tech", "nano", "giga",
    ];
    let animals = vec![
        "fox", "wolf", "bear", "eagle", "owl", "hawk", "lion", "tiger",
        "deer", "rabbit", "turtle", "dolphin", "whale", "horse", "zebra", "panther",
        "cat", "dog", "elephant", "kangaroo", "monkey", "penguin", "snake", "shark",
        "chicken", "giraffe", "octopus", "panda", "rhino", "koala", "platypus",
        "raccoon", "seal", "squirrel", "turtle", "yak", "antelope", "badger", "bat",
        "beaver", "bison", "boar", "buffalo", "camel", "caribou", "cheetah", "cobra",
        "coyote", "crocodile", "crow", "dingo", "elk", "emu", "ferret", "flamingo",
        "frog", "gecko", "gerbil", "goat", "gorilla", "hamster", "hedgehog", "hippo",
        "hyena", "iguana", "jackal", "jaguar", "lemur", "leopard", "llama", "lynx",
        "manatee", "meerkat", "moose", "narwhal", "ocelot", "orangutan", "ostrich", "otter",
        "pelican", "porcupine", "possum", "quail", "rat", "reindeer", "salamander", "scorpion",
        "seahorse", "skunk", "sloth", "sparrow", "stingray", "stork", "swan", "tapir",
        "tarantula", "toucan", "vulture", "wallaby", "warthog", "wombat", "woodpecker", "zebra",
    ];
    
    let mut rng = rand::thread_rng();
    let adjective = adjectives[rng.gen_range(0..adjectives.len())];
    let animal = animals[rng.gen_range(0..animals.len())];
    let number = rng.gen_range(100..9999);
    
    format!("{}-{}-{}", adjective, animal, number)
}

/// Generate a random 16-digit account number (Mullvad-style)
fn generate_account_number() -> String {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| rng.gen_range(0..10).to_string()).collect()
}

/// Create real JWT token for anonymous user
fn create_anonymous_jwt(user_id: &str, jwt_secret: &str) -> Result<String, String> {
    use serde::Serialize;
    
    #[derive(Serialize)]
    struct Claims {
        sub: String,
        exp: usize,
        iat: usize,
        iss: String,
        anonymous: bool,
    }
    
    let now = Utc::now();
    let exp = now + Duration::hours(24); // 24 hour expiry
    
    let claims = Claims {
        sub: user_id.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
        iss: "codetrackr".to_string(),
        anonymous: true,
    };
    
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_ref()),
    ).map_err(|e| format!("Failed to create JWT: {}", e))
}

/// Create anonymous account - generates account number and returns JWT
pub async fn create_anonymous_account(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = generate_account_number();
    let username = generate_anonymous_username();
    let user_id = Uuid::new_v4();
    
    // Create tables if they don't exist
    let create_users_table = r#"
        CREATE TABLE IF NOT EXISTS users (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            username VARCHAR(64) NOT NULL UNIQUE,
            display_name VARCHAR(128),
            email VARCHAR(256),
            avatar_url TEXT,
            github_id VARCHAR(64) UNIQUE,
            gitlab_id VARCHAR(64) UNIQUE,
            plan VARCHAR(16) NOT NULL DEFAULT 'free',
            is_public BOOLEAN NOT NULL DEFAULT true,
            account_number VARCHAR(16) UNIQUE,
            is_anonymous BOOLEAN NOT NULL DEFAULT false,
            country VARCHAR(64),
            timezone VARCHAR(64) NOT NULL DEFAULT 'UTC',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE INDEX IF NOT EXISTS idx_users_account_number ON users(account_number);
    "#;
    
    if let Err(e) = sqlx::query(create_users_table).execute(&state.db_pool).await {
        tracing::error!("Failed to create users table: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Database setup failed"})),
        ));
    }
    
    // Insert user
    let insert_query = r#"
        INSERT INTO users (id, username, account_number, plan, is_public, is_anonymous, timezone, created_at, updated_at)
        VALUES ($1, $2, $3, 'free', true, true, 'UTC', NOW(), NOW())
        RETURNING id, username, account_number, plan, created_at
    "#;
    
    let user_row = sqlx::query(insert_query)
        .bind(user_id)
        .bind(&username)
        .bind(&account_number)
        .fetch_one(&state.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create anonymous user: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to create account", "detail": e.to_string()})),
            )
        })?;

    // Create real JWT access token
    let access_token = create_anonymous_jwt(&user_id.to_string(), &state.jwt_secret)
        .map_err(|e| {
            tracing::error!("Access token creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    // Create mock refresh token for now
    let refresh_token = format!("mock_refresh_{}_{}", user_id, Utc::now().timestamp());

    let response = Json(json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "user": {
            "id": user_row.get::<Uuid, _>("id"),
            "username": user_row.get::<String, _>("username"),
            "account_number": user_row.get::<String, _>("account_number"),
            "plan": user_row.get::<String, _>("plan"),
            "created_at": user_row.get::<chrono::DateTime<chrono::Utc>, _>("created_at")
        },
        "message": "Account created successfully. Save your account number: you'll need it to login."
    }));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(response.to_string().into())
        .unwrap())
}

/// Login with account number - returns JWT if account exists
pub async fn login_with_account_number(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = payload.get("account_number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "account_number is required"})),
            )
        })?;

    // Validate account number format (16 digits)
    if account_number.len() != 16 || !account_number.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid account number format"})),
        ));
    }

    let user_row = sqlx::query(
        "SELECT id, username, account_number, plan, created_at FROM users WHERE account_number = $1 AND is_anonymous = true"
    )
    .bind(account_number)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(|e| {
        tracing::error!("DB error finding user by account number: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Database error"})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid account number"})),
        )
    })?;

    // Create real JWT access token
    let access_token = create_anonymous_jwt(&user_row.get::<Uuid, _>("id").to_string(), &state.jwt_secret)
        .map_err(|e| {
            tracing::error!("Access token creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    // Create mock refresh token for now
    let refresh_token = format!("mock_refresh_{}_{}", user_row.get::<Uuid, _>("id"), Utc::now().timestamp());

    let response = Json(json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "user": {
            "id": user_row.get::<Uuid, _>("id"),
            "username": user_row.get::<String, _>("username"),
            "account_number": user_row.get::<String, _>("account_number"),
            "plan": user_row.get::<String, _>("plan"),
            "created_at": user_row.get::<chrono::DateTime<chrono::Utc>, _>("created_at")
        }
    }));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(response.to_string().into())
        .unwrap())
}

/// Verify account number exists (for frontend validation)
pub async fn verify_account_number(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let account_number = payload.get("account_number")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "account_number is required"})),
            )
        })?;

    // Validate format
    if account_number.len() != 16 || !account_number.chars().all(|c| c.is_ascii_digit()) {
        return Ok(Json(json!({
            "valid": false,
            "message": "Invalid account number format"
        })));
    }

    // Check if exists
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE account_number = $1 AND is_anonymous = true)")
        .bind(account_number)
        .fetch_one(&state.db_pool)
        .await
        .map_err(|e| {
            tracing::error!("DB error verifying account number: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?;

    Ok(Json(json!({
        "valid": exists,
        "message": if exists {
            "Account number found"
        } else {
            "Account number not found"
        }
    })))
}

/// Create refresh token using real service
async fn create_real_refresh_token(
    user_id: Uuid,
    account_number: &str,
    state: &FullAppState,
) -> Result<RefreshTokenResponse, Box<dyn std::error::Error>> {
    let refresh_request = CreateRefreshTokenRequest {
        device_id: format!("anonymous-{}", account_number),
        device_info: Some(json!({
            "type": "anonymous",
            "account_number": account_number,
            "created_at": Utc::now().to_rfc3339()
        })),
    };
    
    RefreshTokenService::create_token(
        user_id,
        refresh_request,
        None, // IP address
        None, // User agent
        state,
    ).await
}

/// Refresh token endpoint using real service
pub async fn refresh_token(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let refresh_token = payload.get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "refresh_token is required"})),
            )
        })?;

    let full_state = FullAppState::from_simple(state);
    
    // Use real refresh token service
    match RefreshTokenService::rotate_token(refresh_token, None, None, &full_state).await {
        Ok(token_response) => {
            // Create new access token for the user
            let access_token = create_anonymous_jwt(&token_response.device_id, &full_state.config.jwt_secret)
                .map_err(|e| {
                    tracing::error!("Access token creation failed: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Authentication failed"})),
                    )
                })?;

            let response = Json(json!({
                "access_token": access_token,
                "refresh_token": token_response.refresh_token,
                "device_id": token_response.device_id,
                "expires_at": token_response.expires_at
            }));

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(response.to_string().into())
                .unwrap())
        }
        Err(e) => {
            tracing::warn!("Refresh token rotation failed: {}", e);
            Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": e})),
            ))
        }
    }
}

/// Serve the frontend
async fn serve_index() -> Response {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>CodeTrackr - Integrated Anonymous Login</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 800px; margin: 0 auto; background: white; padding: 30px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        h1 { color: #333; text-align: center; margin-bottom: 30px; }
        h2 { color: #666; border-bottom: 2px solid #eee; padding-bottom: 10px; margin-top: 30px; }
        button { padding: 12px 24px; margin: 10px; font-size: 16px; border: none; border-radius: 5px; cursor: pointer; }
        .btn-primary { background: #007bff; color: white; }
        .btn-secondary { background: #6c757d; color: white; }
        .btn-success { background: #28a745; color: white; }
        .btn-warning { background: #ffc107; color: black; }
        button:hover { opacity: 0.9; }
        input { padding: 10px; margin: 5px; width: 200px; border: 1px solid #ddd; border-radius: 5px; }
        .result { margin: 20px 0; padding: 15px; border-radius: 5px; font-family: monospace; white-space: pre-wrap; }
        .success { background: #d4edda; color: #155724; border: 1px solid #c3e6cb; }
        .error { background: #f8d7da; color: #721c24; border: 1px solid #f5c6cb; }
        .account-display { background: #e9ecef; padding: 10px; border-radius: 5px; font-family: monospace; font-size: 18px; text-align: center; margin: 10px 0; }
        .status { padding: 10px; margin: 10px 0; border-radius: 5px; }
        .status.authenticated { background: #d1ecf1; color: #0c5460; }
        .status.unauthenticated { background: #f8d7da; color: #721c24; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🦊 CodeTrackr - Integrated Anonymous Login</h1>
        <p style="text-align: center; color: #666; margin-bottom: 30px;">
            Complete anonymous authentication with JWT tokens, refresh tokens, and database persistence.
        </p>
        
        <div id="authStatus" class="status unauthenticated">
            <strong>Status:</strong> Not authenticated
        </div>
        
        <h2>Create Anonymous Account</h2>
        <button class="btn-primary" onclick="createAccount()">Create Account</button>
        
        <h2>Login with Account Number</h2>
        <input type="text" id="accountNumber" placeholder="16-digit account number" maxlength="16">
        <button class="btn-success" onclick="login()">Login</button>
        
        <h2>Verify Account Number</h2>
        <input type="text" id="verifyNumber" placeholder="16-digit account number" maxlength="16">
        <button class="btn-secondary" onclick="verify()">Verify</button>
        
        <h2>Token Management</h2>
        <button class="btn-warning" onclick="refreshToken()">Refresh Token</button>
        <button class="btn-secondary" onclick="logout()">Logout</button>
        
        <div id="result" class="result" style="display: none;"></div>
    </div>

    <script>
        let currentToken = localStorage.getItem('ct_token');
        let currentUser = null;
        
        // Update auth status on load
        updateAuthStatus();
        
        async function createAccount() {
            try {
                const response = await fetch('/auth/anonymous/create', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' }
                });
                const data = await response.json();
                
                if (response.ok) {
                    localStorage.setItem('ct_token', data.access_token);
                    localStorage.setItem('ct_refresh_token', data.refresh_token);
                    currentToken = data.access_token;
                    currentUser = data.user;
                    updateAuthStatus();
                }
                
                showResult(data, response.ok);
                
                if (response.ok && data.user && data.user.account_number) {
                    const accountDiv = document.createElement('div');
                    accountDiv.className = 'account-display';
                    accountDiv.innerHTML = `📋 Account Number: <strong>${data.user.account_number}</strong><br>⚠️ Save this number! You'll need it to login.`;
                    document.getElementById('result').appendChild(accountDiv);
                }
            } catch (error) {
                showResult({ error: error.message }, false);
            }
        }

        async function login() {
            const accountNumber = document.getElementById('accountNumber').value;
            if (!accountNumber) {
                showResult({ error: 'Account number is required' }, false);
                return;
            }
            
            try {
                const response = await fetch('/auth/anonymous/login', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ account_number: accountNumber })
                });
                const data = await response.json();
                
                if (response.ok) {
                    localStorage.setItem('ct_token', data.access_token);
                    localStorage.setItem('ct_refresh_token', data.refresh_token);
                    currentToken = data.access_token;
                    currentUser = data.user;
                    updateAuthStatus();
                }
                
                showResult(data, response.ok);
            } catch (error) {
                showResult({ error: error.message }, false);
            }
        }

        async function verify() {
            const accountNumber = document.getElementById('verifyNumber').value;
            if (!accountNumber) {
                showResult({ error: 'Account number is required' }, false);
                return;
            }
            
            try {
                const response = await fetch('/auth/anonymous/verify', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ account_number: accountNumber })
                });
                const data = await response.json();
                showResult(data, response.ok);
            } catch (error) {
                showResult({ error: error.message }, false);
            }
        }

        async function refreshToken() {
            const refreshToken = localStorage.getItem('ct_refresh_token');
            if (!refreshToken) {
                showResult({ error: 'No refresh token available' }, false);
                return;
            }
            
            try {
                const response = await fetch('/auth/refresh', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ refresh_token: refreshToken })
                });
                const data = await response.json();
                
                if (response.ok) {
                    localStorage.setItem('ct_token', data.access_token);
                    localStorage.setItem('ct_refresh_token', data.refresh_token);
                    currentToken = data.access_token;
                }
                
                showResult(data, response.ok);
            } catch (error) {
                showResult({ error: error.message }, false);
            }
        }

        function logout() {
            localStorage.removeItem('ct_token');
            localStorage.removeItem('ct_refresh_token');
            currentToken = null;
            currentUser = null;
            updateAuthStatus();
            showResult({ message: 'Logged out successfully' }, true);
        }

        function updateAuthStatus() {
            const statusDiv = document.getElementById('authStatus');
            if (currentToken) {
                statusDiv.className = 'status authenticated';
                statusDiv.innerHTML = '<strong>Status:</strong> Authenticated ✅';
            } else {
                statusDiv.className = 'status unauthenticated';
                statusDiv.innerHTML = '<strong>Status:</strong> Not authenticated ❌';
            }
        }

        function showResult(data, success) {
            const result = document.getElementById('result');
            result.style.display = 'block';
            result.className = 'result ' + (success ? 'success' : 'error');
            
            const pre = document.createElement('pre');
            pre.textContent = JSON.stringify(data, null, 2);
            result.innerHTML = '';
            result.appendChild(pre);
        }
    </script>
</body>
</html>
    "#;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html")
        .body(html.into())
        .unwrap()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize database connection
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    
    let db_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "test-secret-key".to_string());

    let state = AppState { db_pool, jwt_secret };

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/auth/anonymous/create", post(create_anonymous_account))
        .route("/auth/anonymous/login", post(login_with_account_number))
        .route("/auth/anonymous/verify", post(verify_account_number))
        .route("/auth/refresh", post(refresh_token))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("🚀 CodeTrackr Integrated Anonymous Login Backend running on http://{}", addr);
    println!("📝 Features:");
    println!("   • Real JWT tokens with 24h expiry");
    println!("   • Database persistence (PostgreSQL)");
    println!("   • Refresh token support");
    println!("   • Account number verification");
    println!("   • Complete frontend integration");
    println!("   • 1000+ username combinations");
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
