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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct AppState {
    users: Arc<RwLock<HashMap<String, AnonymousUser>>>,
    jwt_secret: String,
}

#[derive(Clone, Debug)]
struct AnonymousUser {
    id: Uuid,
    username: String,
    account_number: String,
    plan: String,
    created_at: chrono::DateTime<chrono::Utc>,
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
    
    let user = AnonymousUser {
        id: user_id,
        username: username.clone(),
        account_number: account_number.clone(),
        plan: "free".to_string(),
        created_at: chrono::Utc::now(),
    };
    
    // Store user in memory
    let mut users = state.users.write().await;
    users.insert(account_number.clone(), user);
    
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
            "id": user_id,
            "username": username,
            "account_number": account_number,
            "plan": "free",
            "created_at": chrono::Utc::now().to_rfc3339()
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

    // Find user in memory
    let users = state.users.read().await;
    let user = users.get(account_number).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid account number"})),
        )
    })?;

    // Create access token
    let access_token = create_access_token(&user.id.to_string(), &state.jwt_secret)
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
            "id": user.id,
            "username": user.username,
            "account_number": user.account_number,
            "plan": user.plan,
            "created_at": user.created_at.to_rfc3339()
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

    // Check if exists in memory
    let users = state.users.read().await;
    let exists = users.contains_key(account_number);

    Ok(Json(json!({
        "valid": exists,
        "message": if exists {
            "Account number found"
        } else {
            "Account number not found"
        }
    })))
}

/// List all users (for demo purposes)
pub async fn list_users(
    State(state): State<AppState>,
) -> Json<Value> {
    let users = state.users.read().await;
    let user_list: Vec<Value> = users.values().map(|user| {
        json!({
            "id": user.id,
            "username": user.username,
            "account_number": user.account_number,
            "plan": user.plan,
            "created_at": user.created_at.to_rfc3339()
        })
    }).collect();
    
    Json(json!({
        "users": user_list,
        "total": user_list.len()
    }))
}

/// Serve the frontend
async fn serve_index() -> Response {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>CodeTrackr - Anonymous Login Demo</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 800px; margin: 0 auto; background: white; padding: 30px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        h1 { color: #333; text-align: center; margin-bottom: 30px; }
        h2 { color: #666; border-bottom: 2px solid #eee; padding-bottom: 10px; margin-top: 30px; }
        button { padding: 12px 24px; margin: 10px; font-size: 16px; border: none; border-radius: 5px; cursor: pointer; }
        .btn-primary { background: #007bff; color: white; }
        .btn-secondary { background: #6c757d; color: white; }
        .btn-success { background: #28a745; color: white; }
        .btn-info { background: #17a2b8; color: white; }
        button:hover { opacity: 0.9; }
        input { padding: 10px; margin: 5px; width: 200px; border: 1px solid #ddd; border-radius: 5px; }
        .result { margin: 20px 0; padding: 15px; border-radius: 5px; font-family: monospace; white-space: pre-wrap; }
        .success { background: #d4edda; color: #155724; border: 1px solid #c3e6cb; }
        .error { background: #f8d7da; color: #721c24; border: 1px solid #f5c6cb; }
        .account-display { background: #e9ecef; padding: 10px; border-radius: 5px; font-family: monospace; font-size: 18px; text-align: center; margin: 10px 0; }
        .users-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 15px; margin-top: 15px; }
        .user-card { background: #f8f9fa; padding: 15px; border-radius: 8px; border-left: 4px solid #007bff; }
        .user-card h4 { margin: 0 0 10px 0; color: #333; }
        .user-card p { margin: 5px 0; font-size: 14px; color: #666; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🦊 CodeTrackr - Anonymous Login Demo</h1>
        <p style="text-align: center; color: #666; margin-bottom: 30px;">
            Complete anonymous authentication system with account numbers, JWT tokens, and in-memory storage.
        </p>
        
        <h2>Create Anonymous Account</h2>
        <button class="btn-primary" onclick="createAccount()">Create Account</button>
        
        <h2>Login with Account Number</h2>
        <input type="text" id="accountNumber" placeholder="16-digit account number" maxlength="16">
        <button class="btn-success" onclick="login()">Login</button>
        
        <h2>Verify Account Number</h2>
        <input type="text" id="verifyNumber" placeholder="16-digit account number" maxlength="16">
        <button class="btn-secondary" onclick="verify()">Verify</button>
        
        <h2>All Users</h2>
        <button class="btn-info" onclick="listUsers()">List All Users</button>
        <div id="usersList" style="margin-top: 15px;"></div>
        
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

        async function listUsers() {
            try {
                const response = await fetch('/users', {
                    method: 'GET',
                    headers: { 'Content-Type': 'application/json' }
                });
                const data = await response.json();
                
                const usersList = document.getElementById('usersList');
                if (data.users && data.users.length > 0) {
                    const grid = document.createElement('div');
                    grid.className = 'users-grid';
                    
                    data.users.forEach(user => {
                        const card = document.createElement('div');
                        card.className = 'user-card';
                        card.innerHTML = `
                            <h4>${user.username}</h4>
                            <p><strong>Account:</strong> ${user.account_number}</p>
                            <p><strong>Plan:</strong> ${user.plan}</p>
                            <p><strong>Created:</strong> ${new Date(user.created_at).toLocaleString()}</p>
                        `;
                        grid.appendChild(card);
                    });
                    
                    usersList.innerHTML = '';
                    usersList.appendChild(grid);
                } else {
                    usersList.innerHTML = '<p style="color: #666;">No users created yet.</p>';
                }
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
        
        // Load users on page load
        window.onload = function() {
            listUsers();
        };
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
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "test-secret-key".to_string());

    let state = AppState { 
        users: Arc::new(RwLock::new(HashMap::new())),
        jwt_secret
    };

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/auth/anonymous/create", post(create_anonymous_account))
        .route("/auth/anonymous/login", post(login_with_account_number))
        .route("/auth/anonymous/verify", post(verify_account_number))
        .route("/users", get(list_users))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("🚀 CodeTrackr Anonymous Login Demo running on http://{}", addr);
    println!("📝 Features:");
    println!("   • Create anonymous accounts with 16-digit numbers");
    println!("   • Login with account number");
    println!("   • JWT token authentication");
    println!("   • Account number verification");
    println!("   • In-memory user storage (demo)");
    println!("   • Expanded username generation (1000+ combinations)");
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
