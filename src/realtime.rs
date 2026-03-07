use futures::StreamExt;
use redis::{aio::ConnectionManager, Client};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct RedisPool {
    pub conn: Arc<Mutex<Option<ConnectionManager>>>,
    pub client: Client,
    pub url: String,
}

impl RedisPool {
    pub async fn new(url: &str) -> Result<Self> {
        let client = Client::open(url)?;
        let conn = ConnectionManager::new(client.clone()).await?;
        Ok(Self {
            conn: Arc::new(Mutex::new(Some(conn))),
            client,
            url: url.to_string(),
        })
    }

    /// Returns a live ConnectionManager, reconnecting if the previous one died.
    pub async fn get_conn(&self) -> Result<tokio::sync::MutexGuard<'_, Option<ConnectionManager>>> {
        let mut guard = self.conn.lock().await;
        if guard.is_none() {
            match ConnectionManager::new(self.client.clone()).await {
                Ok(new_conn) => {
                    tracing::info!("Redis: reconnected successfully.");
                    *guard = Some(new_conn);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Redis reconnect failed: {}", e));
                }
            }
        }
        Ok(guard)
    }
}

pub mod ws_handler {
    use axum::{
        extract::{State, WebSocketUpgrade, Query, ws::{WebSocket, Message}},
        response::{Response, IntoResponse, Json},
        http::StatusCode,
    };
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::{Instant, Duration};
    use crate::{AppState, auth::AuthenticatedUser};

    // In-process broadcast channel for real-time updates
    use once_cell::sync::Lazy;
    use tokio::sync::{broadcast, Mutex};

    pub static BROADCAST: Lazy<broadcast::Sender<String>> = Lazy::new(|| {
        broadcast::channel::<String>(1024).0
    });

    /// Tickets WS en memoria: ticket -> (user_id, expiry)
    /// No necesitan Redis — son locales al proceso y de vida muy corta (30s).
    static WS_TICKETS: Lazy<Mutex<HashMap<String, (String, Instant)>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    const TICKET_TTL: Duration = Duration::from_secs(30);

    pub fn publish(msg: String) {
        let _ = BROADCAST.send(msg);
    }

    /// POST /api/v1/ws-ticket
    /// Genera un ticket de un solo uso (TTL 30s) para autenticar la conexión WS
    /// sin exponer el JWT en la query string ni en los logs del servidor.
    pub async fn create_ws_ticket(
        AuthenticatedUser(user): AuthenticatedUser,
        State(_state): State<AppState>,
    ) -> Json<serde_json::Value> {
        use rand::Rng;
        let ticket: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        let expiry = Instant::now() + TICKET_TTL;
        {
            let mut map = WS_TICKETS.lock().await;
            // Limpiar tickets expirados antes de insertar
            map.retain(|_, (_, exp)| *exp > Instant::now());
            map.insert(ticket.clone(), (user.id.to_string(), expiry));
        }

        Json(json!({"ticket": ticket}))
    }

    #[derive(Deserialize)]
    pub struct WsQuery {
        pub ticket: Option<String>,
    }

    pub async fn ws_handler(
        ws: WebSocketUpgrade,
        Query(params): Query<WsQuery>,
        State(_state): State<AppState>,
    ) -> Response {
        let ticket = match params.ticket {
            Some(t) if !t.is_empty() => t,
            _ => return (StatusCode::UNAUTHORIZED, "Missing ticket").into_response(),
        };

        // Consumir ticket (single-use) del store en memoria
        let valid = {
            let mut map = WS_TICKETS.lock().await;
            if let Some((_, expiry)) = map.remove(&ticket) {
                expiry > Instant::now()
            } else {
                false
            }
        };

        if valid {
            ws.on_upgrade(handle_socket)
        } else {
            (StatusCode::UNAUTHORIZED, "Invalid or expired ticket").into_response()
        }
    }

    async fn handle_socket(mut socket: WebSocket) {
        let mut rx = BROADCAST.subscribe();
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(msg) => {
                            if socket.send(Message::Text(msg)).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(Message::Ping(data))) => {
                            let _ = socket.send(Message::Pong(data)).await;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Background task: subscribe to Redis Pub/Sub and forward to in-process broadcast
#[allow(dead_code)]
pub async fn start_redis_subscriber(redis_url: String) {
    tokio::spawn(async move {
        loop {
            match subscribe_loop(&redis_url).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Redis subscriber disconnected: {e}, retrying in 5s...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}

#[allow(dead_code)]
async fn subscribe_loop(url: &str) -> anyhow::Result<()> {
    
    let client = redis::Client::open(url)?;
    let mut pubsub = client.get_async_connection().await?.into_pubsub();
    pubsub.subscribe("codetrackr:updates").await?;

    loop {
        let msg = pubsub.on_message().next().await;
        match msg {
            Some(msg) => {
                let payload: String = msg.get_payload().unwrap_or_default();
                ws_handler::publish(payload);
            }
            None => break,
        }
    }
    Ok(())
}

// Re-export
pub use ws_handler::ws_handler;
