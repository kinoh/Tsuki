mod application;
mod clock;
mod config;
mod db;
mod event;
mod event_store;
mod llm;
mod module_registry;
mod prompts;
mod state;
mod tools;

use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::{broadcast, RwLock};

use crate::config::{load_config, Config, LimitsConfig};
use crate::db::Db;
use crate::event::Event;
use crate::event_store::EventStore;
use crate::module_registry::{ModuleDefinition, ModuleRegistry, ModuleRegistryReader};
use crate::prompts::{load_prompts, write_prompts, PromptOverrides, DEFAULT_PROMPTS_PATH};
use crate::state::{DbStateStore, StateStore};
use crate::tools::{state_tools, StateToolHandler};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) event_store: Arc<EventStore>,
    pub(crate) tx: broadcast::Sender<Event>,
    pub(crate) auth_token: String,
    pub(crate) modules: Modules,
    pub(crate) limits: LimitsConfig,
    pub(crate) prompts: Arc<RwLock<PromptOverrides>>,
    pub(crate) prompts_path: PathBuf,
    pub(crate) decision_instructions: String,
}

#[derive(Clone)]
pub(crate) struct Modules {
    pub(crate) registry: ModuleRegistry,
    pub(crate) runtime: ModuleRuntime,
}

#[derive(Clone)]
pub(crate) struct ModuleRuntime {
    pub(crate) base_instructions: String,
    pub(crate) model: String,
    pub(crate) temperature: Option<f32>,
    pub(crate) max_output_tokens: Option<u32>,
    pub(crate) tools: Vec<async_openai::types::responses::Tool>,
    pub(crate) tool_handler: Arc<dyn crate::llm::ToolHandler>,
    pub(crate) max_tool_rounds: usize,
}

#[derive(Debug, Serialize)]
struct OutboundEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    event: Event,
}

#[derive(Debug, Deserialize)]
struct DebugRunRequest {
    pub(crate) input: String,
    #[serde(default)]
    pub(crate) context_override: Option<String>,
    #[serde(default)]
    pub(crate) submodule_outputs: Option<String>,
    #[serde(default)]
    pub(crate) include_history: Option<bool>,
    #[serde(default)]
    pub(crate) history_cutoff_ts: Option<String>,
    #[serde(default)]
    pub(crate) exclude_event_ids: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) append_input_mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugRunResponse {
    pub(crate) output: String,
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

#[derive(Debug, Deserialize)]
pub(crate) struct DebugImproveTriggerRequest {
    #[serde(default)]
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) reason: Option<String>,
    #[serde(default)]
    pub(crate) feedback_refs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugImproveProposalRequest {
    pub(crate) target: String,
    pub(crate) section: String,
    #[serde(default)]
    pub(crate) reason: Option<String>,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) feedback_refs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugImproveReviewRequest {
    pub(crate) proposal_event_id: String,
    pub(crate) review: String,
    #[serde(default)]
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugImproveResponse {
    pub(crate) proposal_event_id: Option<String>,
    pub(crate) review_event_id: Option<String>,
    pub(crate) auto_approved: bool,
    pub(crate) applied: bool,
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
        .route(
            "/debug/prompts",
            get(debug_get_prompts).post(debug_update_prompts),
        )
        .route("/debug/modules/{name}/run", post(debug_run_module))
        .route("/debug/improve/trigger", post(debug_improve_trigger))
        .route("/debug/improve/proposal", post(debug_improve_proposal))
        .route("/debug/improve/review", post(debug_improve_review))
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

    Modules { registry, runtime }
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
                crate::application::pipeline_service::handle_input(text.to_string(), &state).await;
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
    let desired_modules = payload
        .submodules
        .iter()
        .map(|module| module.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    let active_modules = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    for module in active_modules {
        if !desired_modules.contains(module.name.as_str()) {
            state
                .modules
                .registry
                .upsert(&module.name, &module.instructions, false)
                .await
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        }
    }
    for module in &payload.submodules {
        state
            .modules
            .registry
            .upsert(&module.name, &module.instructions, true)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
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
    let result = crate::application::pipeline_service::run_debug_module(&state, name, payload).await?;
    Ok(Json(result))
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
                .filter(|event| {
                    tags.iter()
                        .all(|tag| event.meta.tags.iter().any(|t| t == tag))
                })
                .collect();
        }
    }

    Ok(Json(DebugEventsResponse { events }))
}

async fn debug_improve_trigger(
    State(state): State<AppState>,
    Json(payload): Json<DebugImproveTriggerRequest>,
) -> Result<Json<DebugImproveResponse>, (StatusCode, String)> {
    let result = crate::application::improve_service::trigger_improvement(&state, payload).await?;
    Ok(Json(result))
}

async fn debug_improve_proposal(
    State(state): State<AppState>,
    Json(payload): Json<DebugImproveProposalRequest>,
) -> Result<Json<DebugImproveResponse>, (StatusCode, String)> {
    let result = crate::application::improve_service::propose_improvement(&state, payload).await?;
    Ok(Json(result))
}

async fn debug_improve_review(
    State(state): State<AppState>,
    Json(payload): Json<DebugImproveReviewRequest>,
) -> Result<Json<DebugImproveResponse>, (StatusCode, String)> {
    let result = crate::application::improve_service::review_improvement(&state, payload).await?;
    Ok(Json(result))
}

async fn debug_ui() -> Html<String> {
    const EMBEDDED: &str = include_str!("../static/debug_ui.html");
    const DEBUG_UI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static/debug_ui.html");
    match tokio::fs::read_to_string(DEBUG_UI_PATH).await {
        Ok(html) => Html(html),
        Err(err) => {
            println!(
                "DEBUG_UI_READ_ERROR path={} error={} (falling back to embedded html)",
                DEBUG_UI_PATH, err
            );
            Html(EMBEDDED.to_string())
        }
    }
}

fn verify_auth(message: &str, expected_token: &str) -> bool {
    let mut parts = message.splitn(2, ':');
    let user = parts.next().unwrap_or("");
    let token = parts.next().unwrap_or("");
    !user.is_empty() && token == expected_token
}

async fn build_effective_prompts(state: &AppState) -> Result<PromptsPayload, (StatusCode, String)> {
    let overrides = state.prompts.read().await.clone();
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

pub(crate) async fn record_event(state: &AppState, event: Event) {
    if let Err(err) = state.event_store.append(&event).await {
        println!("EVENT_STORE_ERROR error={}", err);
    }
    let _ = state.tx.send(event.clone());
    log_event(&event);
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
