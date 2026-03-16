use futures::StreamExt;
use anyhow::Result;

#[derive(Clone)]
pub struct RedisPool {
    pub client: redis::Client,
    pub url: String,
}

impl RedisPool {
    pub async fn new(url: &str) -> Result<Self> {
        let client = redis::Client::open(url)?;
        Ok(Self {
            client,
            url: url.to_string(),
        })
    }

    /// Creates a fresh multiplexed connection on each call.
    pub async fn get_conn(&self) -> Result<redis::aio::MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| anyhow::anyhow!("Redis connection failed: {e}"))
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
                }
            }
            Err(e) => {
                tracing::warn!("[ws] Redis unavailable when creating ticket: {e}");
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
        // GET + DEL secuencial para compatibilidad con Redis < 6.2 (GETDEL requiere 6.2+)
        let valid = match state.redis.get_conn().await {
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
                } else {
                    tracing::warn!("[ws] ticket not found or expired: {}", &ticket[..8]);
                }
                val.is_some()
            },
            Err(e) => {
                tracing::warn!("[ws] Redis connection failed for ticket validation: {e}");
                false
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
