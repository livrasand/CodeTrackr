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
        response::{Response, IntoResponse},
        http::StatusCode,
    };
    use serde::Deserialize;
    use crate::AppState;

    // In-process broadcast channel for real-time updates
    use once_cell::sync::Lazy;
    use tokio::sync::broadcast;

    pub static BROADCAST: Lazy<broadcast::Sender<String>> = Lazy::new(|| {
        broadcast::channel::<String>(1024).0
    });

    pub fn publish(msg: String) {
        let _ = BROADCAST.send(msg);
    }

    #[derive(Deserialize)]
    pub struct WsQuery {
        pub token: Option<String>,
    }

    pub async fn ws_handler(
        ws: WebSocketUpgrade,
        Query(params): Query<WsQuery>,
        State(state): State<AppState>,
    ) -> Response {
        // Fix #4: Require valid JWT to establish WebSocket connection
        let token = match params.token {
            Some(t) if !t.is_empty() => t,
            _ => {
                return (StatusCode::UNAUTHORIZED, "Missing token").into_response();
            }
        };

        if crate::auth::verify_jwt(&token, &state.config.jwt_secret).is_err() {
            return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
        }

        ws.on_upgrade(handle_socket)
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
