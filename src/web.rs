use std::{env, net::SocketAddr, sync::Arc};

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
use reqwest::header::InvalidHeaderValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{
    select,
    sync::{broadcast::Sender, RwLock},
};
use tower_http::cors::CorsLayer;
use tracing::{debug, error, info, warn};

use crate::{
    events::{self, Event, EventComponent},
    messages::{self, MessageRecord, MessageRepository},
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("std::io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Axum error: {0}")]
    Axum(#[from] axum::Error),
    #[error("invalid header value: {0}")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),
    #[error("envvar not set: {0}")]
    EnvVar(&'static str),
}

fn secure_eq(a: &str, b: &str) -> bool {
    let a_bytes: Vec<u8> = a.bytes().collect();
    let b_bytes: Vec<u8> = b.bytes().collect();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    unsafe { memsec::memeq(&a_bytes[0], &b_bytes[0], a_bytes.len()) }
}

async fn logging_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
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
    if req.uri() != "/ws" {
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
        CorsLayer::permissive()
    } else {
        CorsLayer::new()
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/config", get(config))
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
struct ResponseMessage {
    modality: crate::messages::Modality,
    role: crate::messages::Role,
    user: String,
    chat: Value,
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
    let data: Vec<&MessageRecord> = if let Some(n) = params.n {
        repo.get_latest_n(n, params.before)
    } else {
        repo.get_all().iter().map(|m| m).collect()
    };
    let response: Vec<ResponseMessage> = data
        .iter()
        .filter(|m| m.role != crate::messages::Role::System)
        .map(|m| ResponseMessage {
            modality: m.modality,
            role: m.role,
            user: m.user.clone(),
            chat: serde_json::from_str(&m.chat).unwrap_or(Value::String("error".to_string())),
            timestamp: m.timestamp,
        })
        .collect();
    Ok(Json(response))
}

async fn config(State(state): State<Arc<WebState>>) -> Json<Value> {
    Json(state.app_args.clone())
}

async fn notification_test(State(state): State<Arc<WebState>>) -> Result<String, StatusCode> {
    let c = state.as_ref();
    if let Some(s) = &c.sender {
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
    let sender = if let Some(s) = &c.sender {
        s
    } else {
        warn!("not ready");
        return;
    };
    let mut receiver = sender.subscribe();
    let mut authorized_user: Option<String> = None;

    loop {
        select! {
            data = socket.recv() => {
                match data {
                    Some(Ok(message)) => {
                        match message {
                            Message::Text(text) => {
                                if let Some(ref user) = authorized_user {
                                    let _ = sender.send(Event::TextMessage { user: user.to_string(), message: text.to_string()}).map_err(|e| error!("event send error: {}", e));
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
            event = receiver.recv() => {
                if let Some(text) = match event {
                    Ok(Event::AssistantMessage { modality: _, message }) => {
                        Some(message)
                    },
                    Ok(Event::SystemMessage { modality: _, message }) => {
                        Some(format!("[{}] {}", messages::SYSTEM_USER_NAME, message))
                    },
                    Ok(Event::TextMessage { user, message }) => {
                        Some(format!("[{}] {}", user, message))
                    },
                    Err(e) => {
                        warn!("event recv error: {}", e);
                        None
                    },
                    _ => None,
                } {
                    if socket.send(Message::Text(Utf8Bytes::from(text))).await.is_err() {
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
    sender: Option<Sender<Event>>,
    repository: Arc<RwLock<MessageRepository>>,
    auth_token: String,
    app_args: Value,
}

type WebInterface = Arc<WebState>;

impl WebState {
    pub fn new(
        repository: Arc<RwLock<MessageRepository>>,
        port: u16,
        app_args: Value,
    ) -> Result<WebInterface, Error> {
        let auth_token = env::var_os("WEB_AUTH_TOKEN")
            .map(|t| t.to_string_lossy().to_string())
            .and_then(|t| if t.is_empty() { None } else { Some(t) })
            .ok_or(Error::EnvVar("WEB_AUTH_TOKEN"))?;
        Ok(Arc::new(Self {
            port,
            sender: None,
            repository,
            auth_token,
            app_args,
        }))
    }
}

#[async_trait]
impl EventComponent for WebInterface {
    async fn run(&mut self, sender: Sender<Event>) -> Result<(), crate::events::Error> {
        Arc::get_mut(self).map(|c| c.sender = Some(sender));
        serve(Arc::clone(self), self.port)
            .await
            .map_err(|e| events::Error::Component(format!("http: {}", e)))
    }
}
