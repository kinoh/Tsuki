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
use futures::{future::join_all, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, net::SocketAddr, path::PathBuf, sync::Arc};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};
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
    #[serde(default)]
    include_history: Option<bool>,
    #[serde(default)]
    history_cutoff_ts: Option<String>,
    #[serde(default)]
    exclude_event_ids: Option<Vec<String>>,
    #[serde(default)]
    append_input_mode: Option<String>,
}

#[derive(Debug, Serialize)]
struct DebugRunResponse {
    output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppendInputMode {
    AlwaysNew,
    ReuseOpen,
}

impl AppendInputMode {
    fn from_request(value: Option<&str>) -> Self {
        match value {
            Some(raw) if raw.eq_ignore_ascii_case("reuse_open") => Self::ReuseOpen,
            _ => Self::AlwaysNew,
        }
    }
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
struct DebugImproveTriggerRequest {
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    feedback_refs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct DebugImproveProposalRequest {
    target: String,
    section: String,
    #[serde(default)]
    reason: Option<String>,
    content: String,
    #[serde(default)]
    feedback_refs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct DebugImproveReviewRequest {
    proposal_event_id: String,
    review: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct DebugImproveResponse {
    proposal_event_id: Option<String>,
    review_event_id: Option<String>,
    auto_approved: bool,
    applied: bool,
}

#[derive(Debug, Clone)]
struct ModuleOutput {
    name: String,
    text: String,
}

#[derive(Debug, Clone)]
enum PromptTarget {
    Base,
    Decision,
    Submodule(String),
}

impl PromptTarget {
    fn parse(raw: &str) -> Option<Self> {
        let value = raw.trim();
        if value.eq_ignore_ascii_case("base") {
            return Some(Self::Base);
        }
        if value.eq_ignore_ascii_case("decision") {
            return Some(Self::Decision);
        }
        let prefix = "submodule:";
        if let Some(head) = value.get(..prefix.len()) {
            if head.eq_ignore_ascii_case(prefix) {
                let name = value.get(prefix.len()..).unwrap_or("").trim();
                if !name.is_empty() {
                    return Some(Self::Submodule(name.to_string()));
                }
            }
        }
        None
    }
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
    if payload.input.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "input is required".to_string()));
    }
    let include_history = payload.include_history.unwrap_or(true);
    let history_cutoff_ts = payload.history_cutoff_ts.as_deref();
    let excluded_event_ids = payload
        .exclude_event_ids
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    let append_mode = AppendInputMode::from_request(payload.append_input_mode.as_deref());
    maybe_append_debug_input_event(
        &state,
        payload.input.trim(),
        include_history,
        history_cutoff_ts,
        &excluded_event_ids,
        append_mode,
    )
    .await;
    let output = if name == "decision" {
        run_decision_debug(
            &payload.input,
            payload.submodule_outputs.as_deref(),
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            &state,
        )
        .await?
    } else {
        run_submodule_debug(
            &name,
            &payload.input,
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            &state,
        )
        .await?
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
    let target = payload
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual")
        .to_string();
    let reason = payload
        .reason
        .unwrap_or_else(|| "debug trigger".to_string());
    let feedback_refs = payload.feedback_refs.unwrap_or_default();
    let trigger_event = build_event(
        "debug",
        "text",
        json!({
            "phase": "trigger",
            "target": target,
            "reason": reason,
            "feedback_refs": feedback_refs,
        }),
        vec!["improve.trigger".to_string()],
    );
    record_event(&state, trigger_event.clone()).await;
    emit_debug_improve_worklog(
        &state,
        "trigger",
        trigger_event.event_id.as_str(),
        "",
        "trigger emitted",
        None,
    )
    .await;
    Ok(Json(DebugImproveResponse {
        proposal_event_id: None,
        review_event_id: None,
        auto_approved: false,
        applied: false,
    }))
}

async fn debug_improve_proposal(
    State(state): State<AppState>,
    Json(payload): Json<DebugImproveProposalRequest>,
) -> Result<Json<DebugImproveResponse>, (StatusCode, String)> {
    if PromptTarget::parse(&payload.target).is_none() {
        return Err((StatusCode::BAD_REQUEST, "invalid target".to_string()));
    }
    if payload.section.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "section is required".to_string()));
    }
    let proposal_event = build_event(
        "debug",
        "text",
        json!({
            "phase": "proposal",
            "target": payload.target.trim(),
            "section": payload.section.trim(),
            "reason": payload.reason.clone().unwrap_or_else(|| "none".to_string()),
            "content": payload.content,
            "status": "pending",
            "feedback_refs": payload.feedback_refs.unwrap_or_default(),
        }),
        vec!["improve.proposal".to_string()],
    );
    record_event(&state, proposal_event.clone()).await;
    emit_debug_improve_worklog(
        &state,
        "proposal",
        proposal_event.event_id.as_str(),
        proposal_event
            .payload
            .get("reason")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        proposal_event
            .payload
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        proposal_event
            .payload
            .get("section")
            .and_then(|value| value.as_str()),
    )
    .await;

    let is_memory = proposal_event
        .payload
        .get("section")
        .and_then(|value| value.as_str())
        .map(|value| value == "Memory")
        .unwrap_or(false);
    if !is_memory {
        return Ok(Json(DebugImproveResponse {
            proposal_event_id: Some(proposal_event.event_id),
            review_event_id: None,
            auto_approved: false,
            applied: false,
        }));
    }

    let review_event = build_event(
        "debug",
        "text",
        json!({
            "phase": "review",
            "proposal_event_id": proposal_event.event_id.clone(),
            "review": "approval",
            "reason": "auto_approval:Memory",
            "status": "approved",
        }),
        vec!["improve.review".to_string()],
    );
    record_event(&state, review_event.clone()).await;
    emit_debug_improve_worklog(
        &state,
        "review",
        review_event.event_id.as_str(),
        "auto approval",
        "review=approval",
        Some("Memory"),
    )
    .await;

    let apply_result = apply_improve_projection(&state, &proposal_event).await;
    if let Err(err) = apply_result {
        emit_improve_projection_error(&state, review_event.event_id.as_str(), &err).await;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, err));
    }
    Ok(Json(DebugImproveResponse {
        proposal_event_id: Some(proposal_event.event_id),
        review_event_id: Some(review_event.event_id),
        auto_approved: true,
        applied: true,
    }))
}

async fn debug_improve_review(
    State(state): State<AppState>,
    Json(payload): Json<DebugImproveReviewRequest>,
) -> Result<Json<DebugImproveResponse>, (StatusCode, String)> {
    let proposal_event = state
        .event_store
        .get_by_id(payload.proposal_event_id.as_str())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "proposal event not found".to_string(),
            )
        })?;
    if !proposal_event
        .meta
        .tags
        .iter()
        .any(|tag| tag == "improve.proposal")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "event is not improve.proposal".to_string(),
        ));
    }
    let review = normalize_review_value(payload.review.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "review must be approval or rejection".to_string(),
        )
    })?;
    let status = if review == "approval" {
        "approved"
    } else {
        "rejected"
    };
    let review_reason = payload.reason.clone().unwrap_or_else(|| "none".to_string());
    let worklog_input = payload
        .reason
        .as_deref()
        .unwrap_or("manual review")
        .to_string();
    let worklog_output = format!("review={}", review);
    let review_event = build_event(
        "debug",
        "text",
        json!({
            "phase": "review",
            "proposal_event_id": proposal_event.event_id.clone(),
            "review": review,
            "reason": review_reason,
            "status": status,
        }),
        vec!["improve.review".to_string()],
    );
    record_event(&state, review_event.clone()).await;
    emit_debug_improve_worklog(
        &state,
        "review",
        review_event.event_id.as_str(),
        worklog_input.as_str(),
        worklog_output.as_str(),
        proposal_event
            .payload
            .get("section")
            .and_then(|value| value.as_str()),
    )
    .await;

    if review != "approval" {
        return Ok(Json(DebugImproveResponse {
            proposal_event_id: Some(proposal_event.event_id),
            review_event_id: Some(review_event.event_id),
            auto_approved: false,
            applied: false,
        }));
    }

    let apply_result = apply_improve_projection(&state, &proposal_event).await;
    if let Err(err) = apply_result {
        emit_improve_projection_error(&state, review_event.event_id.as_str(), &err).await;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, err));
    }
    Ok(Json(DebugImproveResponse {
        proposal_event_id: Some(proposal_event.event_id),
        review_event_id: Some(review_event.event_id),
        auto_approved: false,
        applied: true,
    }))
}

fn normalize_review_value(value: &str) -> Option<&'static str> {
    if value.eq_ignore_ascii_case("approval") || value.eq_ignore_ascii_case("approved") {
        return Some("approval");
    }
    if value.eq_ignore_ascii_case("rejection") || value.eq_ignore_ascii_case("rejected") {
        return Some("rejection");
    }
    None
}

async fn emit_debug_improve_worklog(
    state: &AppState,
    phase: &str,
    event_id: &str,
    input: &str,
    output: &str,
    section: Option<&str>,
) {
    let worklog_event = build_event(
        "debug",
        "text",
        json!({
            "input": input,
            "output": output,
            "module": "improve",
            "mode": phase,
            "phase": phase,
            "section": section.unwrap_or(""),
            "event_id": event_id,
        }),
        vec![
            "debug".to_string(),
            "worklog".to_string(),
            "module:improve".to_string(),
            format!("mode:{}", phase),
        ],
    );
    record_event(state, worklog_event).await;
}

async fn emit_improve_projection_error(state: &AppState, review_event_id: &str, err: &str) {
    let error_event = build_event(
        "internal",
        "text",
        json!({
            "event_id": review_event_id,
            "text": format!("projection failed: {}", err),
        }),
        vec!["error".to_string()],
    );
    record_event(state, error_event).await;
}

async fn apply_improve_projection(state: &AppState, proposal_event: &Event) -> Result<(), String> {
    let target_raw = payload_str(&proposal_event.payload, "target")
        .ok_or_else(|| "proposal target is required".to_string())?;
    let section = payload_str(&proposal_event.payload, "section")
        .ok_or_else(|| "proposal section is required".to_string())?;
    let content = payload_str(&proposal_event.payload, "content")
        .ok_or_else(|| "proposal content is required".to_string())?;
    let target = PromptTarget::parse(target_raw.as_str())
        .ok_or_else(|| format!("invalid proposal target: {}", target_raw))?;

    let mut overrides = current_prompt_overrides(state).await;
    let current_target_prompt = resolve_target_prompt_text(state, &overrides, &target).await?;
    let next_target_prompt = if section == "Memory" {
        replace_markdown_section_body(current_target_prompt.as_str(), "Memory", content.as_str())?
    } else {
        content.to_string()
    };

    match &target {
        PromptTarget::Base => {
            overrides.base = Some(next_target_prompt);
        }
        PromptTarget::Decision => {
            overrides.decision = Some(next_target_prompt);
        }
        PromptTarget::Submodule(name) => {
            ensure_active_submodule_exists(state, name).await?;
            overrides
                .submodules
                .insert(name.clone(), next_target_prompt);
        }
    }

    write_prompts(&state.prompts_path, &overrides)?;
    *state.prompts.write().await = overrides;
    Ok(())
}

async fn ensure_active_submodule_exists(state: &AppState, name: &str) -> Result<(), String> {
    let modules = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| err.to_string())?;
    if modules.iter().any(|module| module.name == name) {
        return Ok(());
    }
    Err(format!("submodule not found: {}", name))
}

async fn resolve_target_prompt_text(
    state: &AppState,
    overrides: &PromptOverrides,
    target: &PromptTarget,
) -> Result<String, String> {
    match target {
        PromptTarget::Base => Ok(overrides
            .base
            .clone()
            .unwrap_or_else(|| state.modules.runtime.base_instructions.clone())),
        PromptTarget::Decision => Ok(overrides
            .decision
            .clone()
            .unwrap_or_else(|| state.decision_instructions.clone())),
        PromptTarget::Submodule(name) => {
            if let Some(text) = overrides.submodules.get(name) {
                return Ok(text.clone());
            }
            let module = state
                .modules
                .registry
                .list_active()
                .await
                .map_err(|err| err.to_string())?
                .into_iter()
                .find(|item| item.name == *name)
                .ok_or_else(|| format!("submodule not found: {}", name))?;
            Ok(module.instructions)
        }
    }
}

fn payload_str(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn replace_markdown_section_body(
    source: &str,
    section_name: &str,
    body: &str,
) -> Result<String, String> {
    let (start, end) = find_markdown_section_body_range(source, section_name)
        .ok_or_else(|| format!("section not found: {}", section_name))?;
    let mut replacement = body.trim_end_matches('\n').to_string();
    replacement.push('\n');
    let mut output = String::with_capacity(source.len() + replacement.len());
    output.push_str(&source[..start]);
    output.push_str(&replacement);
    output.push_str(&source[end..]);
    Ok(output)
}

fn find_markdown_section_body_range(source: &str, section_name: &str) -> Option<(usize, usize)> {
    #[derive(Clone, Copy)]
    struct Heading {
        level: usize,
        line_start: usize,
        body_start: usize,
    }

    let mut headings: Vec<(Heading, String)> = Vec::new();
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        let hash_count = content.chars().take_while(|ch| *ch == '#').count();
        if hash_count == 0 {
            continue;
        }
        if content.chars().nth(hash_count) != Some(' ') {
            continue;
        }
        let title = content[hash_count + 1..].trim().to_string();
        headings.push((
            Heading {
                level: hash_count,
                line_start,
                body_start: offset,
            },
            title,
        ));
    }
    for (index, (heading, title)) in headings.iter().enumerate() {
        if title != section_name {
            continue;
        }
        let mut end = source.len();
        for (next, _) in headings.iter().skip(index + 1) {
            if next.level <= heading.level {
                end = next.line_start;
                break;
            }
        }
        return Some((heading.body_start, end));
    }
    None
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

async fn maybe_append_debug_input_event(
    state: &AppState,
    input_text: &str,
    include_history: bool,
    cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    append_mode: AppendInputMode,
) {
    let normalized_input = input_text.trim();
    if normalized_input.is_empty() {
        return;
    }
    let should_append = match append_mode {
        AppendInputMode::AlwaysNew => true,
        AppendInputMode::ReuseOpen => {
            if !include_history {
                true
            } else {
                let events = latest_events(state, 1000, cutoff_ts, Some(excluded_event_ids)).await;
                should_append_debug_input_for_reuse_open(normalized_input, &events)
            }
        }
    };
    if !should_append {
        return;
    }
    let event = build_event(
        "user",
        "text",
        json!({ "text": normalized_input }),
        vec!["input".to_string(), "type:message".to_string()],
    );
    record_event(state, event).await;
}

fn should_append_debug_input_for_reuse_open(input_text: &str, events: &[Event]) -> bool {
    let mut saw_decision_after_input = false;
    for event in events {
        if is_decision_event(event) {
            saw_decision_after_input = true;
            continue;
        }
        if !is_user_input_event(event) {
            continue;
        }
        if saw_decision_after_input {
            return true;
        }
        let previous_input = event
            .payload
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim();
        return previous_input != input_text;
    }
    true
}

fn is_user_input_event(event: &Event) -> bool {
    event.source == "user" && event.meta.tags.iter().any(|tag| tag == "input")
}

fn is_decision_event(event: &Event) -> bool {
    event.meta.tags.iter().any(|tag| tag == "decision")
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

async fn build_effective_prompts(state: &AppState) -> Result<PromptsPayload, (StatusCode, String)> {
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
    let history = format_event_history(state, state.limits.submodule_history, None, None).await;
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
            let input = format!("User input: {}\nRecent events:\n{}", input_text, history);
            let instructions = overrides
                .submodules
                .get(&definition.name)
                .cloned()
                .unwrap_or_else(|| definition.instructions.clone());
            let instructions = compose_instructions(&base_instructions, &instructions);
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

async fn run_decision(input_text: &str, modules: &Modules, state: &AppState) -> ModuleOutput {
    let history = format_event_history(state, state.limits.decision_history, None, None).await;
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
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let history = if include_history {
        format_event_history(
            state,
            state.limits.decision_history,
            history_cutoff_ts,
            Some(excluded_event_ids),
        )
        .await
    } else {
        "none".to_string()
    };
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
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let parsed = parse_decision(&response.text);
    let reason_text = parsed.reason.unwrap_or_else(|| "none".to_string());
    let decision_event = build_event(
        "internal",
        "text",
        json!({ "text": format!("decision={} reason={}", parsed.decision, reason_text) }),
        vec!["decision".to_string()],
    );
    record_event(state, decision_event).await;
    emit_debug_module_events(
        state,
        "decision",
        "module_only",
        input_text,
        &context,
        &response,
    )
    .await;
    Ok(response.text)
}

async fn run_submodule_debug(
    name: &str,
    input_text: &str,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let history = if include_history {
        format_event_history(
            state,
            state.limits.submodule_history,
            history_cutoff_ts,
            Some(excluded_event_ids),
        )
        .await
    } else {
        "none".to_string()
    };
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
    let context = format!("User input: {}\nRecent events:\n{}", input_text, history);
    let response = adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let response_event = build_event(
        "internal",
        "text",
        json!({ "text": response.text.clone() }),
        vec!["submodule".to_string(), format!("module:{}", name)],
    );
    record_event(state, response_event).await;
    emit_debug_module_events(state, name, "module_only", input_text, &context, &response).await;
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
    context: &str,
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
            "context": context,
            "output_text": response.text.clone(),
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
                vec![role_tag.to_string(), format!("module:{}", name)],
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

async fn format_event_history(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
) -> String {
    let events = latest_events(state, limit, cutoff_ts, excluded_event_ids).await;
    if events.is_empty() {
        return "none".to_string();
    }
    let mut lines = Vec::with_capacity(events.len() + 1);
    lines.push("ts | role | message".to_string());
    lines.extend(events.iter().map(format_event_line));
    lines.join("\n")
}

async fn latest_events(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
) -> Vec<Event> {
    if limit == 0 {
        return Vec::new();
    }
    match state.event_store.latest(limit).await {
        Ok(events) => events
            .into_iter()
            .filter(|event| !is_debug_event(event))
            .filter(|event| {
                excluded_event_ids
                    .map(|ids| !ids.contains(event.event_id.as_str()))
                    .unwrap_or(true)
            })
            .filter(|event| {
                cutoff_ts
                    .map(|cutoff| event.ts.as_str() >= cutoff)
                    .unwrap_or(true)
            })
            .collect(),
        Err(err) => {
            println!("EVENT_STORE_ERROR error={}", err);
            Vec::new()
        }
    }
}

fn format_event_line(event: &Event) -> String {
    let role = event_role(event);
    let ts = format_local_ts_seconds(&event.ts);
    let payload_text = event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .map(|value| truncate(value, 160))
        .unwrap_or_else(|| truncate(&event.payload.to_string(), 160));
    format!("{} | {} | {}", ts, role, payload_text)
}

fn event_role(event: &Event) -> String {
    let tags = &event.meta.tags;
    if event.source == "user" {
        return "user".to_string();
    }
    if tags.iter().any(|tag| tag == "action") && tags.iter().any(|tag| tag == "response") {
        return "assistant".to_string();
    }
    if tags.iter().any(|tag| tag == "decision") {
        return "decision".to_string();
    }
    if tags.iter().any(|tag| tag == "submodule") {
        if let Some(module_name) = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("module:"))
            .filter(|value| !value.is_empty())
        {
            return format!("submodule:{}", module_name);
        }
        return "submodule".to_string();
    }
    event.source.clone()
}

fn format_local_ts_seconds(ts: &str) -> String {
    let parsed = match OffsetDateTime::parse(ts, &Rfc3339) {
        Ok(value) => value,
        Err(_) => return ts.to_string(),
    };
    let local = match UtcOffset::current_local_offset() {
        Ok(offset) => parsed.to_offset(offset),
        Err(_) => parsed,
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.year(),
        local.month() as u8,
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
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
    let question = extract_field(text, "question=", &["decision=", "reason="]).and_then(|value| {
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
