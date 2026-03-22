use axum::{
    routing::{get, post},
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{Json, Response},
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use uuid::Uuid;
use rand::Rng;
use sqlx::Row;

#[derive(Clone)]
struct AppState {
    db_pool: sqlx::PgPool,
    jwt_secret: String,
}

/// Generate anonymous username (random adjective + animal combination)
fn generate_anonymous_username() -> String {
    let adjectives = vec![
        "silent", "swift", "clever", "bright", "calm", "brave", "wise", "kind",
        "quiet", "noble", "gentle", "bold", "smart", "cool", "warm", "soft",
        "fierce", "grumpy", "lazy", "angry", "happy", "silly", "sneaky", "stupid",
        "tall", "short", "strong", "weak", "powerful", "slow", "fast", "old", "young",
        "tiny", "huge", "giant", "micro", "mega", "ultra", "super", "hyper", "mini", "maxi",
    ];
    let animals = vec![
        "fox", "wolf", "bear", "eagle", "owl", "hawk", "lion", "tiger",
        "deer", "rabbit", "turtle", "dolphin", "whale", "horse", "zebra", "panther",
        "cat", "dog", "elephant", "kangaroo", "monkey", "penguin", "snake", "shark",
        "chicken", "giraffe", "octopus", "panda", "rhino", "koala", "platypus",
        "raccoon", "seal", "squirrel", "turtle", "yak",
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

/// Create JWT access token (simple implementation)
fn create_access_token(user_id: &str, _secret: &str) -> Result<String, String> {
    // Simple mock JWT - in production use proper JWT library
    Ok(format!("mock_jwt_{}_{}", user_id, chrono::Utc::now().timestamp()))
}

/// Create anonymous account - generates account number and returns JWT
pub async fn create_anonymous_account(
    State(state): State<AppState>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = generate_account_number();
    let username = generate_anonymous_username();
    let user_id = Uuid::new_v4();
    
    // Try to create user in database - if table doesn't exist, create it
    let create_table_query = r#"
        CREATE TABLE IF NOT EXISTS anonymous_users (
            id UUID PRIMARY KEY,
            username VARCHAR(255) NOT NULL,
            account_number VARCHAR(16) UNIQUE NOT NULL,
            plan VARCHAR(50) DEFAULT 'free',
            is_anonymous BOOLEAN DEFAULT true,
            created_at TIMESTAMP DEFAULT NOW(),
            updated_at TIMESTAMP DEFAULT NOW()
        )
    "#;
    
    // Create table if it doesn't exist
    if let Err(e) = sqlx::query(create_table_query).execute(&state.db_pool).await {
        tracing::error!("Failed to create table: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Database setup failed"})),
        ));
    }
    
    // Insert user
    let insert_query = r#"
        INSERT INTO anonymous_users (id, username, account_number, plan, is_anonymous, created_at, updated_at)
        VALUES ($1, $2, $3, 'free', true, NOW(), NOW())
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

    // Create access token
    let access_token = create_access_token(&user_id.to_string(), &state.jwt_secret)
        .map_err(|e| {
            tracing::error!("Access token creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    let response = Json(json!({
        "access_token": access_token,
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
        "SELECT id, username, account_number, plan, created_at FROM anonymous_users WHERE account_number = $1"
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

    // Create access token
    let access_token = create_access_token(&user_row.get::<Uuid, _>("id").to_string(), &state.jwt_secret)
        .map_err(|e| {
            tracing::error!("Access token creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Authentication failed"})),
            )
        })?;

    let response = Json(json!({
        "access_token": access_token,
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
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM anonymous_users WHERE account_number = $1)")
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

/// Serve the frontend
async fn serve_index() -> Response {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>CodeTrackr - Anonymous Login</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 600px; margin: 0 auto; background: white; padding: 30px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        h1 { color: #333; text-align: center; }
        h2 { color: #666; border-bottom: 2px solid #eee; padding-bottom: 10px; }
        button { padding: 12px 24px; margin: 10px; font-size: 16px; border: none; border-radius: 5px; cursor: pointer; }
        .btn-primary { background: #007bff; color: white; }
        .btn-secondary { background: #6c757d; color: white; }
        .btn-success { background: #28a745; color: white; }
        button:hover { opacity: 0.9; }
        input { padding: 10px; margin: 5px; width: 200px; border: 1px solid #ddd; border-radius: 5px; }
        .result { margin: 20px 0; padding: 15px; border-radius: 5px; font-family: monospace; white-space: pre-wrap; }
        .success { background: #d4edda; color: #155724; border: 1px solid #c3e6cb; }
        .error { background: #f8d7da; color: #721c24; border: 1px solid #f5c6cb; }
        .account-display { background: #e9ecef; padding: 10px; border-radius: 5px; font-family: monospace; font-size: 18px; text-align: center; margin: 10px 0; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🦊 CodeTrackr - Anonymous Login</h1>
        
        <h2>Create Anonymous Account</h2>
        <button class="btn-primary" onclick="createAccount()">Create Account</button>
        
        <h2>Login with Account Number</h2>
        <input type="text" id="accountNumber" placeholder="16-digit account number" maxlength="16">
        <button class="btn-success" onclick="login()">Login</button>
        
        <h2>Verify Account Number</h2>
        <input type="text" id="verifyNumber" placeholder="16-digit account number" maxlength="16">
        <button class="btn-secondary" onclick="verify()">Verify</button>
        
        <div id="result" class="result" style="display: none;"></div>
    </div>

    <script>
        async function createAccount() {
            try {
                const response = await fetch('/auth/anonymous/create', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' }
                });
                const data = await response.json();
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

        function showResult(data, success) {
            const result = document.getElementById('result');
            result.style.display = 'block';
            result.className = 'result ' + (success ? 'success' : 'error');
            
            // Clear previous content except account display
            const accountDisplay = result.querySelector('.account-display');
            result.innerHTML = '';
            if (accountDisplay) {
                result.appendChild(accountDisplay);
            }
            
            const pre = document.createElement('pre');
            pre.textContent = JSON.stringify(data, null, 2);
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
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("🚀 CodeTrackr Simple Anonymous Login Backend running on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
