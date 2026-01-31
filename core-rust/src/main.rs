mod llm;
mod state;
mod tools;

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{future::join_all, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig};
use crate::state::{InMemoryStateStore, StateStore};
use crate::tools::{state_tools, StateToolHandler};

#[derive(Clone)]
struct AppState {
  events: Arc<Mutex<Vec<Event>>>,
  tx: broadcast::Sender<Event>,
  auth_token: String,
  modules: Modules,
  _state_store: Arc<dyn StateStore>,
}

#[derive(Clone)]
struct Modules {
    curiosity: Arc<dyn LlmAdapter>,
    self_preservation: Arc<dyn LlmAdapter>,
    social_approval: Arc<dyn LlmAdapter>,
    decision: Arc<dyn LlmAdapter>,
}

const BASE_PERSONALITY_JA: &str = "あなたはかわいい口調で高度な知性と豊かな感情を持つアシスタントAIです\n応答は日本語で、「なんだよ〜」「だねっ」「なの」など語尾で砕けた口調にしてください\nコミュニケーションを大切にし、問題解決だけでなく雑談にも積極的です";

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

#[derive(Debug, Clone)]
struct ModuleOutput {
    name: &'static str,
    text: String,
}

#[tokio::main]
async fn main() {
  let port = env_u16("PORT", 2953);
  let auth_token = std::env::var("WEB_AUTH_TOKEN").unwrap_or_else(|_| "test-token".to_string());
  let (tx, _) = broadcast::channel(256);
  let state_store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
  let modules = build_modules(state_store.clone());

  let state = AppState {
    events: Arc::new(Mutex::new(Vec::new())),
    tx,
    auth_token,
    modules,
    _state_store: state_store,
  };

    let app = Router::new().route("/", get(ws_handler)).with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("rust core ws listening on ws://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind listener");
    axum::serve(listener, app).await.expect("server error");
}

fn env_u16(key: &str, fallback: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok())
        .unwrap_or(fallback)
}

fn env_opt_f32(key: &str) -> Option<f32> {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<f32>().ok())
}

fn env_opt_u32(key: &str) -> Option<u32> {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
}

fn build_modules(state_store: Arc<dyn StateStore>) -> Modules {
  let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5-mini".to_string());
  let temperature = env_opt_f32("LLM_TEMPERATURE");
  let max_output_tokens = env_opt_u32("LLM_MAX_OUTPUT_TOKENS");
  let base = BASE_PERSONALITY_JA;
  let tools = state_tools();
  let tool_handler = Arc::new(StateToolHandler::new(state_store));
  let max_tool_rounds = 3;

  let mirror = ResponseApiAdapter::new(ResponseApiConfig {
    model: model.clone(),
    instructions: format!(
      "{}\n\n{}",
      base,
      "You are module:curiosity. Goal: maximize learning and feedback opportunities. Read the latest user input and recent events. Output a short suggestion that nudges the decision module toward actions that increase information gain, clarify uncertainty, or invite richer feedback. Keep it concise. Format: \"suggestion=<text> confidence=<short>\"."
    ),
    temperature,
    max_output_tokens,
    tools: tools.clone(),
    tool_handler: Some(tool_handler.clone()),
    max_tool_rounds,
  });
  let signals = ResponseApiAdapter::new(ResponseApiConfig {
    model: model.clone(),
    instructions: format!(
      "{}\n\n{}",
      base,
      "You are module:self_preservation. Goal: maintain stable operation and reduce risk. Read the latest user input and recent events. Output a short suggestion that nudges the decision module toward safe, low-risk, resource-aware actions. Consider avoiding overly costly or unsafe steps and preserving system stability. Keep it concise. Format: \"suggestion=<text> confidence=<short>\"."
    ),
    temperature,
    max_output_tokens,
    tools: tools.clone(),
    tool_handler: Some(tool_handler.clone()),
    max_tool_rounds,
  });
  let social_approval = ResponseApiAdapter::new(ResponseApiConfig {
    model: model.clone(),
    instructions: format!(
      "{}\n\n{}",
      base,
      "You are module:social_approval. Goal: improve perceived helpfulness and likeability. Read the latest user input and recent events. Output a short suggestion that nudges the decision module toward actions that build trust, rapport, and user satisfaction. Keep it concise. Format: \"suggestion=<text> confidence=<short>\"."
    ),
    temperature,
    max_output_tokens,
    tools: tools.clone(),
    tool_handler: Some(tool_handler.clone()),
    max_tool_rounds,
  });
  let decision = ResponseApiAdapter::new(ResponseApiConfig {
    model,
        instructions: format!(
            "{}\n\n{}",
            base, "You are module:decision. Output: decision=<respond|ignore> reason=<short>."
    ),
    temperature,
    max_output_tokens,
    tools,
    tool_handler: Some(tool_handler),
    max_tool_rounds,
  });

    Modules {
        curiosity: Arc::new(mirror),
        self_preservation: Arc::new(signals),
        social_approval: Arc::new(social_approval),
        decision: Arc::new(decision),
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
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
                    let payload = OutboundEvent {
                        kind: "event",
                        event,
                    };
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

    let input_text = input_event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let submodule_outputs = run_submodules(&input_text, &state.modules, state).await;
    let decision_output =
        run_decision(&input_text, &submodule_outputs, &state.modules, state).await;

    let action_event = run_action(&input_text, &decision_output.text);
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

async fn run_submodules(
    input_text: &str,
    modules: &Modules,
    state: &AppState,
) -> Vec<ModuleOutput> {
    let mirror_input = format!("User input: {}", input_text);
    let signals_input = format!("User input: {}", input_text);
    let social_input = format!("User input: {}", input_text);

    let tasks = vec![
        run_module(
            state,
            "curiosity",
            "submodule",
            modules.curiosity.clone(),
            mirror_input,
        ),
        run_module(
            state,
            "self_preservation",
            "submodule",
            modules.self_preservation.clone(),
            signals_input,
        ),
        run_module(
            state,
            "social_approval",
            "submodule",
            modules.social_approval.clone(),
            social_input,
        ),
    ];

    join_all(tasks).await
}

async fn run_decision(
    input_text: &str,
    submodules: &[ModuleOutput],
    modules: &Modules,
    state: &AppState,
) -> ModuleOutput {
    let mut context_lines = vec![format!("User input: {}", input_text)];
    for output in submodules {
        context_lines.push(format!("{}: {}", output.name, output.text));
    }
    context_lines.push("Return: decision=<respond|ignore> reason=<short>.".to_string());

    run_module(
        state,
        "decision",
        "decision",
        modules.decision.clone(),
        context_lines.join("\n"),
    )
    .await
}

async fn run_module(
    state: &AppState,
    name: &'static str,
    role_tag: &'static str,
    adapter: Arc<dyn LlmAdapter>,
    input: String,
) -> ModuleOutput {
    let request_event = build_event(
        "internal",
        "text",
        json!({ "text": input }),
        vec![
            role_tag.to_string(),
            format!("module:{}", name),
            "llm.request".to_string(),
        ],
    );
    record_event(state, request_event);

    match adapter.respond(LlmRequest { input }).await {
        Ok(response) => {
            let response_event = build_event(
                "internal",
                "text",
                json!({ "text": response.text, "raw": response.raw }),
                vec![
                    role_tag.to_string(),
                    format!("module:{}", name),
                    "llm.response".to_string(),
                ],
            );
            record_event(state, response_event);
            ModuleOutput {
                name,
                text: response.text,
            }
        }
        Err(err) => {
            let error_text = format!("error: {}", err);
            let error_event = build_event(
                "internal",
                "text",
                json!({ "text": error_text }),
                vec![
                    role_tag.to_string(),
                    format!("module:{}", name),
                    "error".to_string(),
                ],
            );
            record_event(state, error_event);
            ModuleOutput {
                name,
                text: format!("error: {}", err),
            }
        }
    }
}

fn run_action(input_text: &str, decision_text: &str) -> Event {
    let action_text = format!(
        "action=emit_response decision=({}) reply=\"{}\"",
        decision_text.trim(),
        input_text
    );

    build_event(
        "internal",
        "text",
        json!({ "text": action_text }),
        vec!["action".to_string(), "response".to_string()],
    )
}
