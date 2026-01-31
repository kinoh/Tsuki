use axum::{
  extract::{
    ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    State,
  },
  response::IntoResponse,
  routing::get,
  Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
  net::SocketAddr,
  sync::{Arc, Mutex},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
  events: Arc<Mutex<Vec<Event>>>,
  tx: broadcast::Sender<Event>,
  auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Event {
  event_id: String,
  ts: String,
  source: String,
  modality: String,
  payload: serde_json::Value,
  meta: EventMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventMeta {
  tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct InputMessage {
  #[serde(default, rename = "type")]
  kind: String,
  text: String,
}

#[derive(Debug, Serialize)]
struct OutboundEvent {
  #[serde(rename = "type")]
  kind: &'static str,
  event: Event,
}

#[tokio::main]
async fn main() {
  let port = env_u16("PORT", 2953);
  let auth_token = std::env::var("WEB_AUTH_TOKEN").unwrap_or_else(|_| "test-token".to_string());
  let (tx, _) = broadcast::channel(256);

  let state = AppState {
    events: Arc::new(Mutex::new(Vec::new())),
    tx,
    auth_token,
  };

  let app = Router::new().route("/", get(ws_handler)).with_state(state);
  let addr = SocketAddr::from(([0, 0, 0, 0], port));

  println!("rust core ws listening on ws://{}", addr);

  let listener = tokio::net::TcpListener::bind(addr)
    .await
    .expect("failed to bind listener");
  axum::serve(listener, app)
    .await
    .expect("server error");
}

fn env_u16(key: &str, fallback: u16) -> u16 {
  std::env::var(key)
    .ok()
    .and_then(|raw| raw.parse::<u16>().ok())
    .unwrap_or(fallback)
}

async fn ws_handler(
  ws: WebSocketUpgrade,
  State(state): State<AppState>,
) -> impl IntoResponse {
  ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
  let auth_text = match socket.recv().await {
    Some(Ok(Message::Text(text))) => text,
    _ => return,
  };

  if !verify_auth(&auth_text, &state.auth_token) {
    let _ = socket
      .send(Message::Close(Some(CloseFrame {
        code: 1008,
        reason: "auth failed".into(),
      })))
      .await;
    return;
  }

  let (mut ws_sender, mut ws_receiver) = socket.split();
  let mut rx = state.tx.subscribe();

  let send_task = tokio::spawn(async move {
    loop {
      match rx.recv().await {
        Ok(event) => {
          let payload = OutboundEvent { kind: "event", event };
          let text = match serde_json::to_string(&payload) {
            Ok(text) => text,
            Err(_) => continue,
          };
          if ws_sender.send(Message::Text(text)).await.is_err() {
            break;
          }
        }
        Err(broadcast::error::RecvError::Lagged(_)) => {
          continue;
        }
        Err(broadcast::error::RecvError::Closed) => break,
      }
    }
  });

  while let Some(Ok(message)) = ws_receiver.next().await {
    match message {
      Message::Text(text) => {
        handle_input(text, &state).await;
      }
      Message::Close(_) => break,
      _ => {}
    }
  }

  send_task.abort();
}

fn verify_auth(message: &str, expected_token: &str) -> bool {
  let mut parts = message.splitn(2, ':');
  let user = parts.next().unwrap_or("");
  let token = parts.next().unwrap_or("");
  !user.is_empty() && token == expected_token
}

async fn handle_input(raw: String, state: &AppState) {
  let parsed: Result<InputMessage, _> = serde_json::from_str(&raw);
  let input = match parsed {
    Ok(message) => message,
    Err(_) => {
      let event = build_event(
        "system",
        "text",
        json!({ "text": "invalid input payload" }),
        vec!["error".to_string()],
      );
      record_event(state, event);
      return;
    }
  };

  let kind = if input.kind.trim().is_empty() {
    "message".to_string()
  } else {
    input.kind.trim().to_string()
  };

  if kind != "message" && kind != "sensory" {
    let event = build_event(
      "system",
      "text",
      json!({ "text": "invalid input type" }),
      vec!["error".to_string()],
    );
    record_event(state, event);
    return;
  }

  let input_event = build_event(
    "user",
    "text",
    json!({ "text": input.text }),
    vec!["input".to_string(), format!("type:{}", kind)],
  );
  record_event(state, input_event.clone());

  let submodule_events = run_submodules(&input_event);
  for event in submodule_events.iter().cloned() {
    record_event(state, event);
  }

  let decision_event = run_decision(&input_event, &submodule_events);
  record_event(state, decision_event.clone());

  let action_event = run_action(&input_event, &decision_event);
  record_event(state, action_event);
}

fn record_event(state: &AppState, event: Event) {
  if let Ok(mut events) = state.events.lock() {
    events.push(event.clone());
  }
  let _ = state.tx.send(event);
}

fn build_event(
  source: &str,
  modality: &str,
  payload: serde_json::Value,
  tags: Vec<String>,
) -> Event {
  Event {
    event_id: Uuid::new_v4().to_string(),
    ts: now_iso8601(),
    source: source.to_string(),
    modality: modality.to_string(),
    payload,
    meta: EventMeta { tags },
  }
}

fn now_iso8601() -> String {
  OffsetDateTime::now_utc()
    .format(&Rfc3339)
    .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn run_submodules(input: &Event) -> Vec<Event> {
  let text = input
    .payload
    .get("text")
    .and_then(|value| value.as_str())
    .unwrap_or("")
    .trim()
    .to_string();

  let mirror_output = format!(
    "module:mirror | prompt=reflect user intent | output=\"{}\"",
    text
  );
  let signal_output = format!(
    "module:signals | prompt=extract quick signals | output=chars:{} words:{}",
    text.chars().count(),
    text.split_whitespace().count()
  );

  vec![
    build_event(
      "internal",
      "text",
      json!({ "text": mirror_output }),
      vec!["submodule".to_string(), "module:mirror".to_string()],
    ),
    build_event(
      "internal",
      "text",
      json!({ "text": signal_output }),
      vec!["submodule".to_string(), "module:signals".to_string()],
    ),
  ]
}

fn run_decision(input: &Event, submodules: &[Event]) -> Event {
  let text = input
    .payload
    .get("text")
    .and_then(|value| value.as_str())
    .unwrap_or("")
    .trim();

  let decision = if text.is_empty() { "ignore" } else { "respond" };
  let reason = format!(
    "decision={} reason=non-empty input submodules={} ",
    decision,
    submodules.len()
  );

  build_event(
    "internal",
    "text",
    json!({ "text": reason }),
    vec!["decision".to_string()],
  )
}

fn run_action(input: &Event, decision: &Event) -> Event {
  let decision_text = decision
    .payload
    .get("text")
    .and_then(|value| value.as_str())
    .unwrap_or("decision=respond");
  let user_text = input
    .payload
    .get("text")
    .and_then(|value| value.as_str())
    .unwrap_or("");

  let action_text = format!(
    "action=emit_response decision=({}) reply=\"{}\"",
    decision_text,
    user_text
  );

  build_event(
    "internal",
    "text",
    json!({ "text": action_text }),
    vec!["action".to_string(), "response".to_string()],
  )
}
