mod config;
mod db;
mod event;
mod event_store;
mod llm;
mod module_registry;
mod prompts;
mod state;
mod clock;
mod tools;

use axum::{
  extract::{
    ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    Path,
    Query,
    State,
  },
  http::StatusCode,
  response::{Html, IntoResponse},
  routing::{get, post},
  Json,
  Router,
};
use futures::{future::join_all, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
  net::SocketAddr,
  path::PathBuf,
  sync::Arc,
};
use tokio::sync::{broadcast, RwLock};

use crate::config::{load_config, Config, LimitsConfig};
use crate::db::Db;
use crate::event::{build_event, Event};
use crate::event_store::EventStore;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig};
use crate::module_registry::{ModuleDefinition, ModuleRegistry, ModuleRegistryReader};
use crate::prompts::{load_prompts, write_prompts, PromptOverrides, DEFAULT_PROMPTS_PATH};
use crate::state::{DbStateStore, StateStore};
use crate::tools::{state_tools, StateToolHandler};

#[derive(Clone)]
struct AppState {
  event_store: Arc<EventStore>,
  tx: broadcast::Sender<Event>,
  auth_token: String,
  modules: Modules,
  limits: LimitsConfig,
  prompts: Arc<RwLock<PromptOverrides>>,
  prompts_path: PathBuf,
  decision_instructions: String,
}

#[derive(Clone)]
struct Modules {
  registry: ModuleRegistry,
  runtime: ModuleRuntime,
}

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

#[derive(Debug, Deserialize)]
struct DebugRunRequest {
    input: String,
    #[serde(default)]
    submodule_outputs: Option<String>,
}

#[derive(Debug, Serialize)]
struct DebugRunResponse {
    output: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct PromptModulePayload {
    name: String,
    instructions: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct PromptsPayload {
    base: String,
    decision: String,
    submodules: Vec<PromptModulePayload>,
}

#[derive(Debug, Deserialize)]
struct DebugEventsQuery {
    tag: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DebugEventsResponse {
    events: Vec<Event>,
}

#[derive(Debug, Clone)]
struct ModuleOutput {
  name: String,
  text: String,
}

#[tokio::main]
async fn main() {
  let config = load_config("config.toml").expect("failed to load config");
  let port = config.server.port;
  let auth_token = std::env::var("WEB_AUTH_TOKEN").expect("WEB_AUTH_TOKEN is required");
  let (tx, _) = broadcast::channel(256);
  let db = Db::connect(&config.db).await.expect("failed to init db");
  let event_store = Arc::new(EventStore::new(db.clone()));
  let prompts_path = PathBuf::from(DEFAULT_PROMPTS_PATH);
  let prompt_overrides = load_prompts(&prompts_path).unwrap_or_default();
  let prompts = Arc::new(RwLock::new(prompt_overrides));
  let emit_event_store = event_store.clone();
  let emit_tx = tx.clone();
  let emit_event = Arc::new(move |event: Event| {
    let event_store = emit_event_store.clone();
    let tx = emit_tx.clone();
    tokio::spawn(async move {
      if let Err(err) = event_store.append(&event).await {
        println!("EVENT_STORE_ERROR error={}", err);
      }
      let _ = tx.send(event.clone());
      log_event(&event);
    });
  });
  let state_store: Arc<dyn StateStore> = Arc::new(DbStateStore::new(db.clone()));
  let module_registry = ModuleRegistry::new(db.clone());
  let defaults = module_defaults_from_config(&config);
  module_registry
    .ensure_defaults(defaults)
    .await
    .expect("failed to seed module registry");
  let modules = build_modules(state_store.clone(), module_registry, &config, emit_event);

  let state = AppState {
    event_store,
    tx,
    auth_token,
    modules,
    limits: config.limits.clone(),
    prompts,
    prompts_path,
    decision_instructions: config.llm.decision_instructions.clone(),
  };

    let app = Router::new()
        .route("/", get(ws_handler))
        .route("/debug/ui", get(debug_ui))
        .route("/debug/prompts", get(debug_get_prompts).post(debug_update_prompts))
        .route("/debug/modules/{name}/run", post(debug_run_module))
        .route("/debug/events", get(debug_events))
        .with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("rust core ws listening on ws://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind listener");
    axum::serve(listener, app).await.expect("server error");
}

fn module_defaults_from_config(config: &Config) -> Vec<ModuleDefinition> {
  config
    .modules
    .iter()
    .map(|module| {
      let mut def = ModuleDefinition::new(&module.name, &module.instructions);
      def.enabled = module.enabled;
      def
    })
    .collect()
}

fn build_modules(
  state_store: Arc<dyn StateStore>,
  registry: ModuleRegistry,
  config: &Config,
  emit_event: Arc<dyn Fn(Event) + Send + Sync>,
) -> Modules {
  let model = config.llm.model.clone();
  let temperature = if config.llm.temperature_enabled {
    Some(config.llm.temperature)
  } else {
    None
  };
  let max_output_tokens = Some(config.llm.max_output_tokens);
  let tools = state_tools();
  let tool_handler = Arc::new(StateToolHandler::new(state_store, emit_event));

  let runtime = ModuleRuntime {
    base_instructions: config.llm.base_personality.clone(),
    model: model.clone(),
    temperature,
    max_output_tokens,
    tools,
    tool_handler,
    max_tool_rounds: config.llm.max_tool_rounds,
  };

  Modules {
    registry,
    runtime,
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

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    println!("WS_CONNECT status=accepted");
    let auth_text = match socket.recv().await {
        Some(Ok(Message::Text(text))) => text,
        Some(Ok(Message::Close(frame))) => {
            println!("WS_CLIENT_CLOSE stage=pre_auth frame={:?}", frame);
            return;
        }
        Some(Ok(_)) => {
            println!("WS_AUTH_FAIL reason=non_text_first_message");
            let _ = socket
                .send(Message::Close(Some(CloseFrame {
                    code: 1002,
                    reason: "auth failed".into(),
                })))
                .await;
            return;
        }
        Some(Err(err)) => {
            println!("WS_ERROR stage=pre_auth error={}", err);
            return;
        }
        _ => return,
    };

    if !verify_auth(&auth_text, &state.auth_token) {
        println!("WS_AUTH_FAIL reason=invalid_token");
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: 1008,
                reason: "auth failed".into(),
            })))
            .await;
        return;
    }
    println!("WS_AUTH_OK");

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
                    if ws_sender.send(Message::Text(text.into())).await.is_err() {
                        println!("WS_SEND_FAIL reason=closed");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    println!("WS_LAGGED");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    while let Some(Ok(message)) = ws_receiver.next().await {
        match message {
            Message::Text(text) => {
                handle_input(text.to_string(), &state).await;
            }
            Message::Close(frame) => {
                println!("WS_CLIENT_CLOSE stage=post_auth frame={:?}", frame);
                break;
            }
            _ => {}
        }
    }

    send_task.abort();
    println!("WS_DISCONNECT");
}

async fn debug_get_prompts(
    State(state): State<AppState>,
) -> Result<Json<PromptsPayload>, (StatusCode, String)> {
    let prompts = build_effective_prompts(&state).await?;
    Ok(Json(prompts))
}

async fn debug_update_prompts(
    State(state): State<AppState>,
    Json(payload): Json<PromptsPayload>,
) -> Result<Json<PromptsPayload>, (StatusCode, String)> {
    let mut submodules = std::collections::HashMap::new();
    for module in &payload.submodules {
        submodules.insert(module.name.clone(), module.instructions.clone());
    }
    let overrides = PromptOverrides {
        base: Some(payload.base.clone()),
        decision: Some(payload.decision.clone()),
        submodules,
    };
    write_prompts(&state.prompts_path, &overrides)
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    *state.prompts.write().await = overrides;
    Ok(Json(payload))
}

async fn debug_run_module(
    Path(name): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<DebugRunRequest>,
) -> Result<Json<DebugRunResponse>, (StatusCode, String)> {
    if payload.input.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "input is required".to_string()));
    }
    let output = if name == "decision" {
        run_decision_debug(
            &payload.input,
            payload.submodule_outputs.as_deref(),
            &state,
        )
        .await?
    } else {
        run_submodule_debug(&name, &payload.input, &state).await?
    };
    Ok(Json(DebugRunResponse { output }))
}

async fn debug_events(
    State(state): State<AppState>,
    Query(query): Query<DebugEventsQuery>,
) -> Result<Json<DebugEventsResponse>, (StatusCode, String)> {
    let limit = query.limit.unwrap_or(200).min(1000);
    let mut events = state
        .event_store
        .latest(limit)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    if let Some(raw_tags) = query.tag {
        let tags = raw_tags
            .split(',')
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !tags.is_empty() {
            events = events
                .into_iter()
                .filter(|event| tags.iter().all(|tag| event.meta.tags.iter().any(|t| t == tag)))
                .collect();
        }
    }

    Ok(Json(DebugEventsResponse { events }))
}

async fn debug_ui() -> Html<&'static str> {
    Html(include_str!("../static/debug_ui.html"))
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
            record_event(state, event).await;
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
        record_event(state, event).await;
        return;
    }

    let input_event = build_event(
        "user",
        "text",
        json!({ "text": input.text }),
        vec!["input".to_string(), format!("type:{}", kind)],
    );
    record_event(state, input_event.clone()).await;

    let input_text = input_event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let _submodule_outputs = run_submodules(&input_text, &state.modules, state).await;
    let _decision_output = run_decision(&input_text, &state.modules, state).await;
}

async fn build_effective_prompts(
    state: &AppState,
) -> Result<PromptsPayload, (StatusCode, String)> {
    let overrides = current_prompt_overrides(state).await;
    let base = overrides
        .base
        .clone()
        .unwrap_or_else(|| state.modules.runtime.base_instructions.clone());
    let decision = overrides
        .decision
        .clone()
        .unwrap_or_else(|| state.decision_instructions.clone());
    let module_defs = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let mut submodules = Vec::new();
    for definition in module_defs {
        let instructions = overrides
            .submodules
            .get(&definition.name)
            .cloned()
            .unwrap_or(definition.instructions);
        submodules.push(PromptModulePayload {
            name: definition.name,
            instructions,
        });
    }
    Ok(PromptsPayload {
        base,
        decision,
        submodules,
    })
}

async fn record_event(state: &AppState, event: Event) {
    if let Err(err) = state.event_store.append(&event).await {
        println!("EVENT_STORE_ERROR error={}", err);
    }
    let _ = state.tx.send(event.clone());
    log_event(&event);
}

async fn run_submodules(
    input_text: &str,
    modules: &Modules,
    state: &AppState,
) -> Vec<ModuleOutput> {
    let history = format_event_history(state, state.limits.submodule_history).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| modules.runtime.base_instructions.clone());
    let module_defs = match modules.registry.list_active().await {
        Ok(list) => list,
        Err(err) => {
            println!("MODULE_REGISTRY_ERROR error={}", err);
            Vec::new()
        }
    };
    let tasks = module_defs
        .into_iter()
        .map(|definition| {
            let input = format!(
                "User input: {}\nRecent events:\n{}",
                input_text,
                history
            );
            let instructions = overrides
                .submodules
                .get(&definition.name)
                .cloned()
                .unwrap_or_else(|| definition.instructions.clone());
            let instructions = compose_instructions(
                &base_instructions,
                &instructions,
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
    let history = format_event_history(state, state.limits.decision_history).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| modules.runtime.base_instructions.clone());
    let decision_instructions = overrides
        .decision
        .clone()
        .unwrap_or_else(|| state.decision_instructions.clone());
    let adapter = ResponseApiAdapter::new(build_config(
        compose_instructions(&base_instructions, &decision_instructions),
        &modules.runtime,
    ));
    let context = format!(
        "Latest user input: {}\nRecent event history:\n{}\nReturn: decision=<respond|ignore|question> reason=<short> question=<text|none>.",
        input_text,
        history
    );
    println!(
        "MODULE_INPUT name=decision role=decision bytes={}",
        context.len()
    );

    let response = match adapter.respond(LlmRequest { input: context }).await {
        Ok(response) => response,
        Err(err) => {
            let error_text = format!("error: {}", err);
            let error_event = build_event(
                "internal",
                "text",
                json!({ "text": error_text }),
                vec!["decision".to_string(), "error".to_string()],
            );
            record_event(state, error_event).await;
            return ModuleOutput {
                name: "decision".to_string(),
                text: format!("error: {}", err),
            };
        }
    };

    let parsed = parse_decision(&response.text);
    let reason_text = parsed.reason.unwrap_or_else(|| "none".to_string());
    let decision_event = build_event(
        "internal",
        "text",
        json!({ "text": format!("decision={} reason={}", parsed.decision, reason_text) }),
        vec!["decision".to_string()],
    );
    record_event(state, decision_event).await;

    ModuleOutput {
        name: "decision".to_string(),
        text: response.text,
    }
}

async fn run_decision_debug(
    input_text: &str,
    submodule_outputs_raw: Option<&str>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let history = format_event_history(state, state.limits.decision_history).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| state.modules.runtime.base_instructions.clone());
    let decision_instructions = overrides
        .decision
        .clone()
        .unwrap_or_else(|| state.decision_instructions.clone());
    let adapter = ResponseApiAdapter::new(build_config(
        compose_instructions(&base_instructions, &decision_instructions),
        &state.modules.runtime,
    ));
    let submodule_outputs = parse_submodule_outputs(submodule_outputs_raw);
    let submodule_section = if submodule_outputs.is_empty() {
        "Submodule outputs (user provided): none".to_string()
    } else {
        let lines = submodule_outputs
            .into_iter()
            .map(|(name, text)| format!("- {}: {}", name, text))
            .collect::<Vec<_>>()
            .join("\n");
        format!("Submodule outputs (user provided):\n{}", lines)
    };
    let context = format!(
        "Latest user input: {}\n{}\nRecent event history:\n{}\nReturn: decision=<respond|ignore|question> reason=<short> question=<text|none>.",
        input_text, submodule_section, history
    );
    let response = adapter
        .respond(LlmRequest { input: context })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    emit_debug_module_events(state, "decision", "module_only", input_text, &response).await;
    Ok(response.text)
}

async fn run_submodule_debug(
    name: &str,
    input_text: &str,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let history = format_event_history(state, state.limits.submodule_history).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| state.modules.runtime.base_instructions.clone());
    let module_defs = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let definition = module_defs
        .into_iter()
        .find(|definition| definition.name == name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "module not found".to_string()))?;
    let instructions = overrides
        .submodules
        .get(&definition.name)
        .cloned()
        .unwrap_or(definition.instructions);
    let adapter = ResponseApiAdapter::new(build_config(
        compose_instructions(&base_instructions, &instructions),
        &state.modules.runtime,
    ));
    let context = format!(
        "User input: {}\nRecent events:\n{}",
        input_text,
        history
    );
    let response = adapter
        .respond(LlmRequest { input: context })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    emit_debug_module_events(state, name, "module_only", input_text, &response).await;
    Ok(response.text)
}

fn parse_submodule_outputs(raw: Option<&str>) -> Vec<(String, String)> {
    let raw = match raw {
        Some(value) => value,
        None => return Vec::new(),
    };
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                return None;
            }
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

async fn emit_debug_module_events(
    state: &AppState,
    module: &str,
    mode: &str,
    input_text: &str,
    response: &crate::llm::LlmResponse,
) {
    let worklog_event = build_event(
        "debug",
        "text",
        json!({
            "input": input_text,
            "output": response.text.clone(),
            "module": module,
            "mode": mode,
        }),
        vec![
            "debug".to_string(),
            "worklog".to_string(),
            format!("module:{}", module),
            format!("mode:{}", mode),
        ],
    );
    record_event(state, worklog_event).await;

    let raw_event = build_event(
        "debug",
        "text",
        json!({
            "raw": response.raw.clone(),
            "module": module,
            "mode": mode,
        }),
        vec![
            "debug".to_string(),
            "llm.raw".to_string(),
            format!("module:{}", module),
            format!("mode:{}", mode),
        ],
    );
    record_event(state, raw_event).await;
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
                json!({ "text": response.text }),
                vec![
                    role_tag.to_string(),
                    format!("module:{}", name),
                ],
            );
            record_event(state, response_event).await;
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
            record_event(state, error_event).await;
            ModuleOutput {
                name: name.clone(),
                text: format!("error: {}", err),
            }
        }
    }
}

async fn current_prompt_overrides(state: &AppState) -> PromptOverrides {
    state.prompts.read().await.clone()
}

async fn format_event_history(state: &AppState, limit: usize) -> String {
    let events = latest_events(state, limit).await;
    if events.is_empty() {
        return "none".to_string();
    }
    events
        .iter()
        .map(format_event_line)
        .collect::<Vec<_>>()
        .join("\n")
}

async fn latest_events(state: &AppState, limit: usize) -> Vec<Event> {
    if limit == 0 {
        return Vec::new();
    }
    match state.event_store.latest(limit).await {
        Ok(events) => events
            .into_iter()
            .filter(|event| !is_debug_event(event))
            .collect(),
        Err(err) => {
            println!("EVENT_STORE_ERROR error={}", err);
            Vec::new()
        }
    }
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

fn is_debug_event(event: &Event) -> bool {
    event.meta.tags.iter().any(|tag| tag == "debug")
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
