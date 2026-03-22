use futures::StreamExt;
use anyhow::Result;

use redis::aio::ConnectionManager;

#[derive(Clone)]
pub struct RedisPool {
    pub manager: ConnectionManager,
    pub url: String,
}

impl RedisPool {
    pub async fn new(url: &str) -> Result<Self> {
        let client = redis::Client::open(url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self {
            manager,
            url: url.to_string(),
        })
    }

    /// Returns a clone of the ConnectionManager.
    /// ConnectionManager handles reconnection automatically.
    pub async fn get_conn(&self) -> Result<ConnectionManager> {
        Ok(self.manager.clone())
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
    use crate::{AppState, auth::AuthenticatedUser};

    // In-process broadcast channel for real-time updates
    use std::sync::OnceLock;
    use tokio::sync::broadcast;

    // In-process ticket store as fallback when Redis is unavailable
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub static TICKET_STORE: OnceLock<Arc<RwLock<HashMap<String, (u64, u64)>>>> = OnceLock::new();

    fn get_ticket_store() -> Arc<RwLock<HashMap<String, (u64, u64)>>> {
        TICKET_STORE.get_or_init(|| {
            Arc::new(RwLock::new(HashMap::new()))
        }).clone()
    }

    // Clean expired tickets from memory store
    async fn clean_expired_tickets() {
        let store = get_ticket_store();
        let mut tickets = store.write().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        tickets.retain(|_, (_, expires)| *expires > now);
    }

    pub static BROADCAST: OnceLock<broadcast::Sender<String>> = OnceLock::new();

    fn get_broadcast() -> &'static broadcast::Sender<String> {
        BROADCAST.get_or_init(|| {
            broadcast::channel::<String>(1024).0
        })
    }

    const TICKET_TTL_SECS: u64 = 30;

    pub fn publish(msg: String) {
        let _ = get_broadcast().send(msg);
    }

    /// POST /api/v1/ws-ticket
    /// Genera un ticket de un solo uso (TTL 30s) almacenado en Redis.
    /// Escala horizontalmente entre instancias — cualquier nodo puede validar el ticket.
    pub async fn create_ws_ticket(
        AuthenticatedUser(user): AuthenticatedUser,
        State(state): State<AppState>,
    ) -> Json<serde_json::Value> {
        use rand::Rng;
        let ticket: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        let key = format!("ws_ticket:{}", ticket);
        let mut stored_in_memory = false;
        
        // Try Redis first
        match state.redis.get_conn().await {
            Ok(mut conn) => {
                let result: Result<(), _> = redis::cmd("SETEX")
                    .arg(&key)
                    .arg(TICKET_TTL_SECS)
                    .arg(user.id.to_string())
                    .query_async(&mut conn)
                    .await;
                if let Err(e) = result {
                    tracing::warn!("[ws] Failed to store ticket in Redis: {e}");
                    // Fallback to memory store
                    stored_in_memory = true;
                }
            }
            Err(e) => {
                tracing::warn!("[ws] Redis unavailable when creating ticket: {e}");
                stored_in_memory = true;
            }
        }

        // Fallback to memory store if Redis failed
        if stored_in_memory {
            let store = get_ticket_store();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let expires = now + TICKET_TTL_SECS;
            
            let mut tickets = store.write().await;
            tickets.insert(ticket.clone(), (user.id.as_u128() as u64, expires));
            
            // Clean expired tickets periodically
            if tickets.len() % 10 == 0 {
                drop(tickets);
                clean_expired_tickets().await;
            }
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
        State(state): State<AppState>,
    ) -> Response {
        let ticket = match params.ticket {
            Some(t) if !t.is_empty() => t,
            _ => return (StatusCode::UNAUTHORIZED, "Missing ticket").into_response(),
        };

        let key = format!("ws_ticket:{}", ticket);
        let mut valid = false;
        
        // Try Redis first
        match state.redis.get_conn().await {
            Ok(mut conn) => {
                let val: Option<String> = redis::cmd("GET")
                    .arg(&key)
                    .query_async(&mut conn)
                    .await
                    .unwrap_or(None);
                if val.is_some() {
                    let _: () = redis::cmd("DEL")
                        .arg(&key)
                        .query_async(&mut conn)
                        .await
                        .unwrap_or(());
                    valid = true;
                } else {
                    tracing::warn!("[ws] ticket not found or expired in Redis: {}", &ticket[..8]);
                }
            },
            Err(e) => {
                tracing::warn!("[ws] Redis connection failed for ticket validation: {e}");
                // Try memory store as fallback
                let store = get_ticket_store();
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                
                let mut tickets = store.write().await;
                if let Some((_user_id, expires)) = tickets.remove(&ticket) {
                    if expires > now {
                        valid = true;
                    } else {
                        tracing::warn!("[ws] ticket expired in memory store: {}", &ticket[..8]);
                    }
                } else {
                    tracing::warn!("[ws] ticket not found in memory store: {}", &ticket[..8]);
                }
                
                // Clean expired tickets
                tickets.retain(|_, (_, expires)| *expires > now);
            },
        };

        if valid {
            ws.on_upgrade(handle_socket)
        } else {
            (StatusCode::UNAUTHORIZED, "Invalid or expired ticket").into_response()
        }
    }

    async fn handle_socket(mut socket: WebSocket) {
        let mut rx = get_broadcast().subscribe();
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
    let mut pubsub = client.get_async_pubsub().await?;
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
