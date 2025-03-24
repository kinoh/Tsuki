use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{HeaderValue, StatusCode},
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
}

async fn serve(state: Arc<WebState>, port: u16) -> Result<(), Error> {
    let app = Router::new()
        .route("/", get(root))
        .route("/messages", get(messages))
        .route("/ws", any(ws_handler))
        .layer(CorsLayer::new().allow_origin("http://localhost:1420".parse::<HeaderValue>()?))
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

    loop {
        select! {
            data = socket.recv() => {
                match data {
                    Some(Ok(message)) => {
                        match message {
                            Message::Text(text) => {
                                let v: Vec<&str> = text.splitn(2, ' ').collect();
                                let (user, content) = if v.len() == 2 {
                                    (v[0], v[1])
                                } else {
                                    ("", text.as_ref())
                                };
                                let _ = sender.send(Event::TextMessage { user: user.to_string(), message: content.to_string() }).map_err(|e| println!("event send error: {}", e));
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
                match event {
                    Ok(Event::AssistantText { message }) => {
                        if socket.send(Message::Text(Utf8Bytes::from(message))).await.is_err() {
                            return;
                        }
                    },
                    Err(e) => println!("event recv error: {}", e),
                    _ => (),
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
}

type WebInterface = Arc<WebState>;

impl WebState {
    pub fn new(repository: Arc<RwLock<MessageRepository>>, port: u16) -> WebInterface {
        Arc::new(Self {
            port,
            sender: None,
            repository,
        })
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
