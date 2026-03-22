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

#[derive(Clone)]
struct AppState {
    // Minimal state for anonymous login testing
}

/// Generate anonymous username (random adjective + animal combination)
fn generate_anonymous_username() -> String {
    let adjectives = vec![
        "silent", "swift", "clever", "bright", "calm", "brave", "wise", "kind",
        "quiet", "noble", "gentle", "bold", "smart", "cool", "warm", "soft"
    ];
    let animals = vec![
        "fox", "wolf", "bear", "eagle", "owl", "hawk", "lion", "tiger",
        "deer", "rabbit", "turtle", "dolphin", "whale", "horse", "zebra", "panther"
    ];
    
    let mut rng = rand::thread_rng();
    let adj = adjectives[rng.gen_range(0..adjectives.len())];
    let animal = animals[rng.gen_range(0..animals.len())];
    let number = rng.gen_range(100..999);
    
    format!("{}_{}_{}", adj, animal, number)
}

/// Generate a random 16-digit account number (Mullvad-style)
fn generate_account_number() -> String {
    let mut rng = rand::thread_rng();
    (0..16).map(|_| rng.gen_range(0..10).to_string()).collect()
}

/// Create anonymous account - generates account number and returns JWT
pub async fn create_anonymous_account(
    State(_state): State<AppState>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let account_number = generate_account_number();
    let username = generate_anonymous_username();
    let user_id = Uuid::new_v4();
    
    // Mock JWT tokens (in real implementation, these would be properly signed)
    let access_token = format!("mock_access_token_{}", user_id);
    let refresh_token = format!("mock_refresh_token_{}", user_id);

    let response = Json(json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "user": {
            "id": user_id,
            "username": username,
            "account_number": account_number,
            "plan": "free",
            "created_at": "2024-01-01T00:00:00Z"
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
    State(_state): State<AppState>,
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

    // Mock successful login (in real implementation, would check database)
    let user_id = Uuid::new_v4();
    let access_token = format!("mock_access_token_{}", user_id);
    let refresh_token = format!("mock_refresh_token_{}", user_id);

    let response = Json(json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "user": {
            "id": user_id,
            "username": format!("user-{}", account_number),
            "account_number": account_number,
            "plan": "free",
            "created_at": "2024-01-01T00:00:00Z"
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
    State(_state): State<AppState>,
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

    // Mock validation (in real implementation, would check database)
    Ok(Json(json!({
        "valid": true,
        "message": "Account number found"
    })))
}

/// Serve the frontend
async fn serve_index() -> Response {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>CodeTrackr - Anonymous Login Test</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; }
        .container { max-width: 600px; margin: 0 auto; }
        button { padding: 10px 20px; margin: 10px; font-size: 16px; }
        input { padding: 8px; margin: 5px; width: 200px; }
        .result { margin: 20px 0; padding: 10px; background: #f0f0f0; }
        .success { background: #d4edda; }
        .error { background: #f8d7da; }
    </style>
</head>
<body>
    <div class="container">
        <h1>CodeTrackr - Anonymous Login Test</h1>
        
        <h2>Create Anonymous Account</h2>
        <button onclick="createAccount()">Create Account</button>
        
        <h2>Login with Account Number</h2>
        <input type="text" id="accountNumber" placeholder="16-digit account number" maxlength="16">
        <button onclick="login()">Login</button>
        
        <h2>Verify Account Number</h2>
        <input type="text" id="verifyNumber" placeholder="16-digit account number" maxlength="16">
        <button onclick="verify()">Verify</button>
        
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
            result.innerHTML = '<pre>' + JSON.stringify(data, null, 2) + '</pre>';
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
async fn main() {
    let state = AppState {};

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/auth/anonymous/create", post(create_anonymous_account))
        .route("/auth/anonymous/login", post(login_with_account_number))
        .route("/auth/anonymous/verify", post(verify_account_number))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("🚀 Anonymous login server running on http://{}", addr);
    
    axum::serve(listener, app).await.unwrap();
}
