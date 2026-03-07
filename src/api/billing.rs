use axum::{
    extract::State,
    response::Json,
    http::{StatusCode, HeaderMap},
    body::Bytes,
};
use serde::Deserialize;
use serde_json::{json, Value};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{AppState, auth::AuthenticatedUser};

type HmacSha256 = Hmac<Sha256>;

// ── Create Checkout Session ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CheckoutRequest {
    pub price_id: String,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
}

pub async fn create_checkout_session(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
    Json(body): Json<CheckoutRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let secret_key = std::env::var("STRIPE_SECRET_KEY")
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Stripe not configured"}))))?;

    let frontend_url = &state.config.frontend_url;
    let success_url = body.success_url.unwrap_or_else(|| format!("{}/billing/success", frontend_url));
    let cancel_url = body.cancel_url.unwrap_or_else(|| format!("{}/billing/cancel", frontend_url));

    // Fetch or create Stripe customer
    let customer_id = match get_or_create_customer(&user, &secret_key).await {
        Ok(id) => id,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e})))),
    };

    // Store customer_id if not already saved
    if user.stripe_customer_id.is_none() {
        let _ = sqlx::query(
            "UPDATE users SET stripe_customer_id = $1 WHERE id = $2"
        )
        .bind(&customer_id)
        .bind(user.id)
        .execute(&state.db.pool)
        .await;
    }

    // Create checkout session
    let user_id_str = user.id.to_string();
    let client = reqwest::Client::new();
    let params = [
        ("mode", "subscription"),
        ("customer", customer_id.as_str()),
        ("line_items[0][price]", body.price_id.as_str()),
        ("line_items[0][quantity]", "1"),
        ("success_url", success_url.as_str()),
        ("cancel_url", cancel_url.as_str()),
        ("metadata[user_id]", user_id_str.as_str()),
    ];

    let resp = client
        .post("https://api.stripe.com/v1/checkout/sessions")
        .basic_auth(&secret_key, Some(""))
        .form(&params)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    let status = resp.status();
    let data: Value = resp.json().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    if !status.is_success() {
        let msg = data["error"]["message"].as_str().unwrap_or("Stripe error").to_string();
        return Err((StatusCode::BAD_GATEWAY, Json(json!({"error": msg}))));
    }

    let session_url = data["url"].as_str().unwrap_or("").to_string();
    let session_id = data["id"].as_str().unwrap_or("").to_string();

    Ok(Json(json!({
        "session_id": session_id,
        "url": session_url,
    })))
}

// ── Get Billing Status ────────────────────────────────────────────────────────

pub async fn get_billing_status(
    AuthenticatedUser(user): AuthenticatedUser,
    State(_state): State<AppState>,
) -> Json<Value> {
    Json(json!({
        "plan": user.plan,
        "stripe_customer_id": user.stripe_customer_id,
        "stripe_subscription_id": user.stripe_subscription_id,
        "plan_expires_at": user.plan_expires_at,
    }))
}

// ── Stripe Config (public key for frontend) ───────────────────────────────────

pub async fn get_billing_config() -> Json<Value> {
    let publishable_key = std::env::var("STRIPE_PUBLISHABLE_KEY").unwrap_or_default();
    let price_id = std::env::var("STRIPE_PRICE_ID").unwrap_or_default();
    Json(json!({ "publishable_key": publishable_key, "price_id": price_id }))
}

// ── Create Portal Session (manage subscription) ───────────────────────────────

pub async fn create_portal_session(
    AuthenticatedUser(user): AuthenticatedUser,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let secret_key = std::env::var("STRIPE_SECRET_KEY")
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Stripe not configured"}))))?;

    let customer_id = user.stripe_customer_id.ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(json!({"error": "No active subscription"})))
    })?;

    let return_url = format!("{}/settings", state.config.frontend_url);

    let client = reqwest::Client::new();
    let params = [
        ("customer", customer_id.as_str()),
        ("return_url", return_url.as_str()),
    ];

    let resp = client
        .post("https://api.stripe.com/v1/billing_portal/sessions")
        .basic_auth(&secret_key, Some(""))
        .form(&params)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    let status = resp.status();
    let data: Value = resp.json().await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(json!({"error": e.to_string()}))))?;

    if !status.is_success() {
        let msg = data["error"]["message"].as_str().unwrap_or("Stripe error").to_string();
        return Err((StatusCode::BAD_GATEWAY, Json(json!({"error": msg}))));
    }

    Ok(Json(json!({ "url": data["url"] })))
}

// ── Stripe Webhook ────────────────────────────────────────────────────────────

pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET")
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Webhook secret not configured"}))))?;

    let sig_header = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({"error": "Missing stripe-signature"}))))?;

    // Verify signature
    verify_stripe_signature(&body, sig_header, &webhook_secret)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;

    let event: Value = serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))))?;

    let event_type = event["type"].as_str().unwrap_or("");

    match event_type {
        "checkout.session.completed" => {
            handle_checkout_completed(&state, &event).await?;
        }
        "customer.subscription.updated" => {
            handle_subscription_updated(&state, &event).await?;
        }
        "customer.subscription.deleted" => {
            handle_subscription_deleted(&state, &event).await?;
        }
        _ => {}
    }

    Ok(Json(json!({"received": true})))
}

// ── Webhook event handlers ────────────────────────────────────────────────────

async fn handle_checkout_completed(
    state: &AppState,
    event: &Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    let session = &event["data"]["object"];
    let user_id_str = session["metadata"]["user_id"].as_str().unwrap_or("");
    let subscription_id = session["subscription"].as_str().unwrap_or("");
    let customer_id = session["customer"].as_str().unwrap_or("");

    if user_id_str.is_empty() || subscription_id.is_empty() {
        return Ok(());
    }

    let user_id = uuid::Uuid::parse_str(user_id_str)
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid user_id"}))))?;

    sqlx::query(
        "UPDATE users SET plan = 'pro', stripe_subscription_id = $1, stripe_customer_id = $2, updated_at = NOW() WHERE id = $3"
    )
    .bind(subscription_id)
    .bind(customer_id)
    .bind(user_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    tracing::info!("User {} upgraded to Pro (sub: {})", user_id, subscription_id);
    Ok(())
}

async fn handle_subscription_updated(
    state: &AppState,
    event: &Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    let sub = &event["data"]["object"];
    let subscription_id = sub["id"].as_str().unwrap_or("");
    let status = sub["status"].as_str().unwrap_or("");

    // active / trialing = pro; past_due / canceled / unpaid = free
    let plan = if matches!(status, "active" | "trialing") { "pro" } else { "free" };

    let current_period_end = sub["current_period_end"].as_i64();
    let expires_at = current_period_end
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|dt: chrono::DateTime<chrono::Utc>| dt);

    sqlx::query(
        "UPDATE users SET plan = $1, plan_expires_at = $2, updated_at = NOW() WHERE stripe_subscription_id = $3"
    )
    .bind(plan)
    .bind(expires_at)
    .bind(subscription_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    tracing::info!("Subscription {} updated to plan={}", subscription_id, plan);
    Ok(())
}

async fn handle_subscription_deleted(
    state: &AppState,
    event: &Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    let sub = &event["data"]["object"];
    let subscription_id = sub["id"].as_str().unwrap_or("");

    sqlx::query(
        "UPDATE users SET plan = 'free', stripe_subscription_id = NULL, plan_expires_at = NULL, updated_at = NOW() WHERE stripe_subscription_id = $1"
    )
    .bind(subscription_id)
    .execute(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    tracing::info!("Subscription {} deleted, user downgraded to free", subscription_id);
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn get_or_create_customer(
    user: &crate::models::User,
    secret_key: &str,
) -> Result<String, String> {
    if let Some(ref cid) = user.stripe_customer_id {
        return Ok(cid.clone());
    }

    let client = reqwest::Client::new();
    let email = user.email.clone().unwrap_or_default();
    let user_id_str = user.id.to_string();
    let params = [
        ("email", email.as_str()),
        ("name", user.display_name.as_deref().unwrap_or(&user.username)),
        ("metadata[user_id]", user_id_str.as_str()),
    ];

    let resp = client
        .post("https://api.stripe.com/v1/customers")
        .basic_auth(secret_key, Some(""))
        .form(&params)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let data: Value = resp.json().await.map_err(|e| e.to_string())?;

    data["id"].as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| data["error"]["message"].as_str().unwrap_or("Stripe error").to_string())
}

fn verify_stripe_signature(
    payload: &[u8],
    sig_header: &str,
    secret: &str,
) -> Result<(), String> {
    // Parse timestamp and signatures from header
    // Format: t=timestamp,v1=signature1,v1=signature2,...
    let mut timestamp = "";
    let mut signatures: Vec<&str> = vec![];

    for part in sig_header.split(',') {
        if let Some(ts) = part.strip_prefix("t=") {
            timestamp = ts;
        } else if let Some(sig) = part.strip_prefix("v1=") {
            signatures.push(sig);
        }
    }

    if timestamp.is_empty() || signatures.is_empty() {
        return Err("Invalid signature header".to_string());
    }

    // Fix #8: Reject webhooks older than 300 seconds to prevent replay attacks
    let ts: i64 = timestamp.parse().map_err(|_| "Invalid timestamp in signature".to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if (now - ts).abs() > 300 {
        return Err("Webhook timestamp too old or too far in the future".to_string());
    }

    // Signed payload = timestamp + "." + raw body
    let signed_payload = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| "Invalid webhook secret".to_string())?;
    mac.update(signed_payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    if signatures.iter().any(|&s| s == expected) {
        Ok(())
    } else {
        Err("Signature mismatch".to_string())
    }
}
