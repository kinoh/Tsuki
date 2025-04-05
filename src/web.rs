use std::{env, sync::Arc};

use async_trait::async_trait;
use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
        Request, State,
    },
    http::{self, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{any, get},
    Json, Router,
};
use reqwest::header::InvalidHeaderValue;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tokio::{select, sync::broadcast::Sender, sync::RwLock};
use tower_http::cors::CorsLayer;

use crate::{
    events::{self, Event, EventComponent},
    messages::MessageRepository,
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
        .route("/messages", get(messages))
        .route("/ws", any(ws_handler))
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;

    println!("start listen port={}", port);
    axum::serve(listener, app).await?;

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
}

async fn messages(
    State(state): State<Arc<WebState>>,
) -> Result<Json<Vec<ResponseMessage>>, StatusCode> {
    let reepo = state.repository.read().await;
    let response: Vec<ResponseMessage> = reepo
        .get_all()
        .iter()
        .filter(|m| m.role != crate::messages::Role::System)
        .map(|m| ResponseMessage {
            modality: m.modality,
            role: m.role,
            user: m.user.clone(),
            chat: serde_json::from_str(&m.chat).unwrap_or(Value::String("error".to_string())),
        })
        .collect();
    Ok(Json(response))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<WebState>>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<WebState>) {
    let c = state.as_ref();
    let sender = if let Some(s) = &c.sender {
        s
    } else {
        println!("not ready");
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
                                    let _ = sender.send(Event::TextMessage { user: user.to_string(), message: text.to_string()}).map_err(|e| println!("event send error: {}", e));
                                } else {
                                    let mut parts = text.splitn(2, ':');
                                    let (user, token) = match (parts.next(), parts.next()) {
                                        (Some(u), Some(t)) => (u, t),
                                        _ => {
                                            println!("invalid auth");
                                            return;
                                        }
                                    };
                                    if !secure_eq(token, &state.auth_token) {
                                        println!("invalid auth token");
                                        return;
                                    }
                                    authorized_user = Some(user.to_string());
                                }
                            }
                            Message::Close(_) => {
                                println!("stream closed gracefully");
                                return;
                            }
                            _ => println!("unexpected message type")
                        }
                    }
                    Some(Err(e)) => println!("recv error: {}", e),
                    None => {
                        println!("stream closed");
                        return;
                    }
                }
            },
            event = receiver.recv() => {
                if let Some(text) = match event {
                    Ok(Event::AssistantText { message }) => {
                        Some(message)
                    },
                    Ok(Event::CodeExecutionRequest { code }) => {
                        Some(code)
                    },
                    Ok(Event::TextMessage { user, message }) => {
                        Some(format!("[{}] {}", user, message))
                    },
                    Err(e) => {
                        println!("event recv error: {}", e);
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
}

type WebInterface = Arc<WebState>;

impl WebState {
    pub fn new(
        repository: Arc<RwLock<MessageRepository>>,
        port: u16,
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
