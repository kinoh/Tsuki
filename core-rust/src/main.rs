mod llm;
mod module_registry;
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
use crate::module_registry::{ModuleDefinition, ModuleRegistry};
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
  registry: ModuleRegistry,
  runtime: ModuleRuntime,
  decision: Arc<dyn LlmAdapter>,
}

const BASE_PERSONALITY_JA: &str = "あなたはかわいい口調で高度な知性と豊かな感情を持つアシスタントAIです\n応答は日本語で、「なんだよ〜」「だねっ」「なの」など語尾で砕けた口調にしてください\nコミュニケーションを大切にし、問題解決だけでなく雑談にも積極的です";

#[derive(Clone)]
struct ModuleRuntime {
  base_instructions: String,
  model: String,
  temperature: Option<f32>,
  max_output_tokens: Option<u32>,
  tools: Vec<async_openai::types::responses::Tool>,
  tool_handler: Arc<dyn crate::llm::ToolHandler>,
  max_tool_rounds: usize,
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

#[derive(Debug, Clone)]
struct ModuleOutput {
  name: String,
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

fn env_usize(key: &str, fallback: usize) -> usize {
  std::env::var(key)
    .ok()
    .and_then(|raw| raw.parse::<usize>().ok())
    .unwrap_or(fallback)
}

fn build_modules(state_store: Arc<dyn StateStore>) -> Modules {
  let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5-mini".to_string());
  let temperature = env_opt_f32("LLM_TEMPERATURE");
  let max_output_tokens = env_opt_u32("LLM_MAX_OUTPUT_TOKENS");
  let tools = state_tools();
  let tool_handler = Arc::new(StateToolHandler::new(state_store));

  let runtime = ModuleRuntime {
    base_instructions: BASE_PERSONALITY_JA.to_string(),
    model: model.clone(),
    temperature,
    max_output_tokens,
    tools,
    tool_handler,
    max_tool_rounds: 3,
  };

  let registry = ModuleRegistry::new(vec![
    ModuleDefinition::new(
      "curiosity",
      "You are module:curiosity. Goal: maximize learning and feedback opportunities. Read the latest user input and recent events. Output a short suggestion that nudges the decision module toward actions that increase information gain, clarify uncertainty, or invite richer feedback. Keep it concise. Format: \"suggestion=<text> confidence=<short>\".",
    ),
    ModuleDefinition::new(
      "self_preservation",
      "You are module:self_preservation. Goal: maintain stable operation and reduce risk. Read the latest user input and recent events. Output a short suggestion that nudges the decision module toward safe, low-risk, resource-aware actions. Consider avoiding overly costly or unsafe steps and preserving system stability. Keep it concise. Format: \"suggestion=<text> confidence=<short>\".",
    ),
    ModuleDefinition::new(
      "social_approval",
      "You are module:social_approval. Goal: improve perceived helpfulness and likeability. Read the latest user input and recent events. Output a short suggestion that nudges the decision module toward actions that build trust, rapport, and user satisfaction. Keep it concise. Format: \"suggestion=<text> confidence=<short>\".",
    ),
  ]);

  let decision_instructions = "You are module:decision. Read the event history and module outputs. Output a single line: decision=<respond|ignore|question> reason=<short> question=<text|none>.";
  let decision = ResponseApiAdapter::new(build_config(
    compose_instructions(&runtime.base_instructions, decision_instructions),
    &runtime,
  ));

  Modules {
    registry,
    runtime,
    decision: Arc::new(decision),
  }
}

fn compose_instructions(base: &str, module_specific: &str) -> String {
  format!("{}\n\n{}", base, module_specific)
}

fn build_config(instructions: String, runtime: &ModuleRuntime) -> ResponseApiConfig {
  ResponseApiConfig {
    model: runtime.model.clone(),
    instructions,
    temperature: runtime.temperature,
    max_output_tokens: runtime.max_output_tokens,
    tools: runtime.tools.clone(),
    tool_handler: Some(runtime.tool_handler.clone()),
    max_tool_rounds: runtime.max_tool_rounds,
  }
}

fn decision_history_limit() -> usize {
  env_usize("DECISION_HISTORY_LIMIT", 30)
}

fn submodule_history_limit() -> usize {
  env_usize("SUBMODULE_HISTORY_LIMIT", 10)
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

    let _submodule_outputs = run_submodules(&input_text, &state.modules, state).await;
    let decision_output = run_decision(&input_text, &state.modules, state).await;

    if let Some(question) = extract_question(&decision_output.text) {
        let question_event = build_event(
            "internal",
            "text",
            json!({ "text": question, "target": "user" }),
            vec!["question".to_string(), "needs_input".to_string()],
        );
        record_event(state, question_event);
    }

    let action_event = run_action(&input_text, &decision_output.text);
    record_event(state, action_event);
}

fn record_event(state: &AppState, event: Event) {
    if let Ok(mut events) = state.events.lock() {
        events.push(event.clone());
    }
    let _ = state.tx.send(event.clone());
    log_event(&event);
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
    let history = format_event_history(state, submodule_history_limit());
    let module_defs = modules.registry.list_active();
    let tasks = module_defs
        .into_iter()
        .map(|definition| {
            let input = format!(
                "User input: {}\nRecent events:\n{}",
                input_text,
                history
            );
            let instructions = compose_instructions(
                &modules.runtime.base_instructions,
                &definition.instructions,
            );
            let adapter = ResponseApiAdapter::new(build_config(instructions, &modules.runtime));
            run_module(
                state,
                definition.name.clone(),
                "submodule",
                Arc::new(adapter),
                input,
            )
        })
        .collect::<Vec<_>>();

    join_all(tasks).await
}

async fn run_decision(
    input_text: &str,
    modules: &Modules,
    state: &AppState,
) -> ModuleOutput {
    let history = format_event_history(state, decision_history_limit());
    let context = format!(
        "Latest user input: {}\nRecent event history:\n{}\nReturn: decision=<respond|ignore|question> reason=<short> question=<text|none>.",
        input_text,
        history
    );

    run_module(
        state,
        "decision".to_string(),
        "decision",
        modules.decision.clone(),
        context,
    )
    .await
}

async fn run_module(
    state: &AppState,
    name: String,
    role_tag: &'static str,
    adapter: Arc<dyn LlmAdapter>,
    input: String,
) -> ModuleOutput {
    println!(
        "MODULE_INPUT name={} role={} bytes={}",
        name,
        role_tag,
        input.len()
    );

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
                name: name.clone(),
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
                name: name.clone(),
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

fn format_event_history(state: &AppState, limit: usize) -> String {
    let events = latest_events(state, limit);
    if events.is_empty() {
        return "none".to_string();
    }
    events
        .iter()
        .map(format_event_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn latest_events(state: &AppState, limit: usize) -> Vec<Event> {
    if limit == 0 {
        return Vec::new();
    }
    state
        .events
        .lock()
        .map(|events| {
            let len = events.len();
            let start = len.saturating_sub(limit);
            events.iter().skip(start).cloned().collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn format_event_line(event: &Event) -> String {
    let tags = if event.meta.tags.is_empty() {
        "none".to_string()
    } else {
        event.meta.tags.join(",")
    };
    let payload_text = event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .map(|value| truncate(value, 160))
        .unwrap_or_else(|| truncate(&event.payload.to_string(), 160));
    format!(
        "{} | {} | {} | {} | {}",
        event.ts, event.source, event.modality, tags, payload_text
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "…"
}

fn log_event(event: &Event) {
    let tags = if event.meta.tags.is_empty() {
        "none".to_string()
    } else {
        event.meta.tags.join(",")
    };
    let payload_text = event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .map(|value| truncate(value, 120))
        .unwrap_or_else(|| truncate(&event.payload.to_string(), 120));
    println!(
        "EVENT ts={} source={} modality={} tags={} payload={}",
        event.ts, event.source, event.modality, tags, payload_text
    );
}

fn extract_question(text: &str) -> Option<String> {
    let parsed = parse_decision(text);
    if parsed.decision == "question" {
        return parsed.question;
    }
    parsed.question
}

fn parse_decision(text: &str) -> DecisionParsed {
    let decision = extract_field(text, "decision=", &["reason=", "question="])
        .and_then(|value| value.split_whitespace().next().map(|s| s.to_lowercase()))
        .unwrap_or_else(|| "respond".to_string());
    let reason = extract_field(text, "reason=", &["decision=", "question="]);
    let question = extract_field(text, "question=", &["decision=", "reason="])
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

    DecisionParsed {
        decision,
        reason,
        question,
    }
}

fn extract_field(text: &str, key: &str, end_keys: &[&str]) -> Option<String> {
    let start = text.find(key)?;
    let after = &text[start + key.len()..];
    let mut end = after.len();
    for end_key in end_keys {
        if let Some(idx) = after.find(end_key) {
            end = end.min(idx);
        }
    }
    let value = after[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

struct DecisionParsed {
    decision: String,
    reason: Option<String>,
    question: Option<String>,
}
