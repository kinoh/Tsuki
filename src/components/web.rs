use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use async_trait::async_trait;
use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
        ConnectInfo, Query, Request, State,
    },
    http::{self, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{any, get, post},
    Json, Router,
};
use reqwest::{header::InvalidHeaderValue, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::{select, sync::RwLock};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

use crate::common::{
    broadcast::{self, IdentifiedBroadcast},
    chat::{ChatInput, ChatInputMessage, ChatOutput, ChatOutputMessage, Modality},
    events::{self, Event, EventComponent},
    message::{MessageRecord, MessageRecordChat, ASSISTANT_NAME},
    repository::Repository,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("std::io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Axum error: {0}")]
    Axum(#[from] axum::Error),
    #[error("invalid header value: {0}")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
    #[error("Auth token is empty")]
    AuthTokenIsEmpty,
}

fn secure_eq(a: &str, b: &str) -> bool {
    let a_bytes: Vec<u8> = a.bytes().collect();
    let b_bytes: Vec<u8> = b.bytes().collect();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    unsafe { memsec::memeq(&a_bytes[0], &b_bytes[0], a_bytes.len()) }
}

fn is_local(req: &Request) -> bool {
    req.headers()
        .get("Host")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|h| h.starts_with("localhost:") || h.starts_with("127.0.0.1:"))
}

async fn logging_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let path = req
        .uri()
        .path_and_query()
        .map_or(String::new(), |p| p.to_string());
    let method = req.method().to_string();

    let response = next.run(req).await;

    info!(
        client = addr.to_string(),
        method = method,
        path = path,
        status = response.status().as_str()
    );

    response
}

async fn auth_middleware(
    State(state): State<Arc<WebState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Websocket has its own authorization
    // CORS preflight request (using OPTIONS) must not restricted
    let bypass = (req.uri() == "/ws") || (req.method() == Method::OPTIONS) || is_local(&req);
    if !bypass {
        let auth_header = req.headers_mut().get(http::header::AUTHORIZATION);
        let auth_header = match auth_header {
            Some(header) => header.to_str().map_err(|_| StatusCode::FORBIDDEN)?,
            None => return Err(StatusCode::FORBIDDEN),
        };
        let mut parts = auth_header.split_whitespace();
        let token = match (parts.next(), parts.next()) {
            (Some("Bearer"), Some(t)) => t,
            _ => return Err(StatusCode::FORBIDDEN),
        };
        if !secure_eq(token, &state.auth_token) {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(next.run(req).await)
}

async fn serve(state: Arc<WebState>, port: u16) -> Result<(), Error> {
    let cors = if cfg!(debug_assertions) {
        info!("Permissive CORS policy for development");
        CorsLayer::permissive()
    } else {
        CorsLayer::new()
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/metrics", get(metrics))
        .route("/metadata", get(metadata))
        .route("/messages", get(messages))
        .route("/notification/test", post(notification_test))
        .route("/ws", any(ws_handler))
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(middleware::from_fn(logging_middleware))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;

    info!(port = port, "start listen");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn root() -> &'static str {
    "Hello, World!"
}

#[derive(Serialize)]
enum Role {
    Assistant,
    User,
}

#[derive(Serialize)]
struct ResponseMessage {
    role: Role,
    user: String,
    chat: Value,
    token_usage: u32,
    timestamp: u64,
}

#[derive(Debug, Deserialize)]
struct MessagesParams {
    n: Option<usize>,
    before: Option<u64>,
}

async fn messages(
    State(state): State<Arc<WebState>>,
    Query(params): Query<MessagesParams>,
) -> Result<Json<Vec<ResponseMessage>>, StatusCode> {
    let repo = state.repository.read().await;
    let data: Vec<&MessageRecord> = repo.messages(params.n, params.before);
    let response: Result<Vec<ResponseMessage>, serde_json::Error> = data
        .iter()
        .map(|m: &&MessageRecord| {
            Ok(ResponseMessage {
                role: match m.chat {
                    MessageRecordChat::Input(_) => Role::User,
                    MessageRecordChat::Output(_) => Role::Assistant,
                },
                user: m.user(),
                chat: m.json_chat()?,
                token_usage: m.usage.clone(),
                timestamp: m.timestamp,
            })
        })
        .collect();
    Ok(Json(response.map_err(|e| {
        error!("serialization error: {}", e);
        StatusCode::from_u16(500).unwrap()
    })?))
}

async fn metadata(State(state): State<Arc<WebState>>) -> Json<Value> {
    Json(json!({
        "revision": std::env::var("GIT_HASH").unwrap_or(String::from("-")),
        "args": state.app_args.clone(),
    }))
}

async fn metrics(State(state): State<Arc<WebState>>) -> Json<Value> {
    let repo = state.repository.read().await;
    let messages = repo.messages(None, None);
    let mut sessions: Vec<&String> = messages.iter().map(|r| &r.session).collect();
    sessions.dedup();

    Json(json!({
        "total_usage": messages.iter().map(|r| r.usage).sum::<u32>(),
        "total_messages": messages.len(),
        "total_sessions": sessions.len(),
    }))
}

async fn notification_test(State(state): State<Arc<WebState>>) -> Result<String, StatusCode> {
    let c = state.as_ref();
    if let Some(s) = &c.broadcast {
        s.send(Event::Notify {
            content: format!("通知テストだよ🔎 ({})", chrono::Utc::now().format("%+")),
        })
        .map_err(|e| {
            error!("event send error: {}", e);
            StatusCode::from_u16(500).unwrap()
        })?;
        Ok("ok".to_string())
    } else {
        warn!("not ready");
        Err(StatusCode::from_u16(503).unwrap())
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<WebState>>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<WebState>) {
    let c = state.as_ref();
    let broadcast = if let Some(b) = &c.broadcast {
        b
    } else {
        warn!("not ready");
        return;
    };
    let mut broadcast = broadcast.participate();
    let mut authorized_user: Option<String> = None;

    loop {
        select! {
            data = socket.recv() => {
                match data {
                    Some(Ok(message)) => {
                        match message {
                            Message::Text(text) => {
                                if let Some(ref user) = authorized_user {
                                    let _ = broadcast.send(Event::TextMessage { user: user.to_string(), message: text.to_string()}).map_err(|e| error!("event send error: {}", e));
                                } else {
                                    let mut parts = text.splitn(2, ':');
                                    let (user, token) = match (parts.next(), parts.next()) {
                                        (Some(u), Some(t)) => (u, t),
                                        (u, _) => {
                                            info!(user = u, "invalid auth");
                                            return;
                                        }
                                    };
                                    if !secure_eq(token, &state.auth_token) {
                                        info!("invalid auth token");
                                        return;
                                    }
                                    info!(user = user, "authenticated");
                                    authorized_user = Some(user.to_string());
                                }
                            }
                            Message::Close(_) => {
                                info!("stream closed gracefully");
                                return;
                            }
                            _ => debug!("unexpected message type")
                        }
                    }
                    Some(Err(e)) => warn!("recv error: {}", e),
                    None => {
                        info!("stream closed");
                        return;
                    }
                }
            },
            event = broadcast.recv() => {
                if let Some(response) = match event {
                    Ok(Event::AssistantMessage { modality, message, usage }) => {
                        Some(ResponseMessage {
                            role: Role::Assistant,
                            user: ASSISTANT_NAME.to_string(),
                            token_usage: usage,
                            chat: serde_json::to_value(vec![
                                ChatOutput::Message(ChatOutputMessage { feeling: None, activity: None, modality, content: Some(message) })
                            ]).unwrap_or(Value::String("<serialization failed>".to_string())),
                            timestamp: SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
                        })
                    },
                    Ok(Event::TextMessage { user, message }) => {
                        Some(ResponseMessage {
                            role: Role::User,
                            user: user.clone(),
                            token_usage: 0,
                            chat: serde_json::to_value(vec![
                                ChatInput::Message(ChatInputMessage { user: user.clone(), modality: Modality::Text, content: message })
                            ]).unwrap_or(Value::String("<serialization failed>".to_string())),
                            timestamp: SystemTime::now()
                                .duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
                        })
                    },
                    Err(e) => {
                        warn!("event recv error: {}", e);
                        None
                    },
                    _ => None,
                } {
                    let response_json = serde_json::to_string(&response).unwrap_or("<serialization failed>".to_string());
                    if socket.send(Message::Text(Utf8Bytes::from(response_json))).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct WebState {
    port: u16,
    broadcast: Option<IdentifiedBroadcast<Event>>,
    repository: Arc<RwLock<Repository>>,
    auth_token: String,
    app_args: Value,
}

type WebInterface = Arc<WebState>;

impl WebState {
    pub fn new(
        repository: Arc<RwLock<Repository>>,
        port: u16,
        auth_token: &str,
        app_args: Value,
    ) -> Result<WebInterface, Error> {
        if auth_token.is_empty() {
            return Err(Error::AuthTokenIsEmpty);
        }
        Ok(Arc::new(Self {
            port,
            broadcast: None,
            repository,
            auth_token: auth_token.to_string(),
            app_args,
        }))
    }
}

#[async_trait]
impl EventComponent for WebInterface {
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        Arc::get_mut(self).map(|c| c.broadcast = Some(broadcast.participate()));
        serve(Arc::clone(self), self.port)
            .await
            .map_err(|e| events::Error::Component(format!("http: {}", e)))
    }
}
