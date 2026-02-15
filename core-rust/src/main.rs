mod activation_concept_graph;
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
    http::{header::CONTENT_TYPE, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        Html, IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::{broadcast, RwLock};

use crate::activation_concept_graph::{ActivationConceptGraphStore, ConceptGraphStore};
use crate::config::{load_config, Config, InputConfig, LimitsConfig, RouterConfig};
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
    pub(crate) state_store: Arc<dyn StateStore>,
    pub(crate) activation_concept_graph: Arc<dyn ConceptGraphStore>,
    pub(crate) modules: Modules,
    pub(crate) limits: LimitsConfig,
    pub(crate) router: RouterConfig,
    pub(crate) input: InputConfig,
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
    source: Option<String>,
    module: Option<String>,
    since_ts: Option<String>,
    until_ts: Option<String>,
    around_event_id: Option<String>,
    around_window: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DebugEventsResponse {
    events: Vec<Event>,
}

#[derive(Debug, Deserialize)]
struct DebugConceptSearchQuery {
    q: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DebugConceptGraphQueriesQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DebugConceptSearchResponse {
    items: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct DebugConceptGraphQueryItem {
    event_id: String,
    ts: String,
    query_terms: Vec<String>,
    limit: Option<usize>,
    result_concepts: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct DebugConceptGraphQueriesResponse {
    items: Vec<DebugConceptGraphQueryItem>,
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
    let arousal_tau_ms = std::env::var("AROUSAL_TAU_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(86_400_000.0);
    let activation_concept_graph = Arc::new(
        ActivationConceptGraphStore::connect(
            std::env::var("MEMGRAPH_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string()),
            std::env::var("MEMGRAPH_USER").unwrap_or_default(),
            std::env::var("MEMGRAPH_PASSWORD").unwrap_or_default(),
            arousal_tau_ms,
        )
        .await
        .expect("failed to connect activation concept graph store"),
    );
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
        state_store,
        activation_concept_graph,
        modules,
        limits: config.limits.clone(),
        router: config.router.clone(),
        input: config.input.clone(),
        prompts,
        prompts_path,
        decision_instructions: config.llm.decision_instructions.clone(),
    };

    let app = Router::new()
        .route("/", get(ws_handler))
        .route("/debug/styles/{name}", get(debug_style))
        .route("/debug/ui", get(debug_ui))
        .route("/debug/monitor", get(debug_monitor_ui))
        .route("/debug/concept-graph/ui", get(debug_concept_graph_ui))
        .route(
            "/debug/concept-graph/health",
            get(debug_concept_graph_health),
        )
        .route("/debug/concept-graph/stats", get(debug_concept_graph_stats))
        .route(
            "/debug/concept-graph/concepts",
            get(debug_concept_graph_concepts),
        )
        .route(
            "/debug/concept-graph/concepts/{name}",
            get(debug_concept_graph_concept_detail),
        )
        .route(
            "/debug/concept-graph/episodes",
            get(debug_concept_graph_episodes),
        )
        .route(
            "/debug/concept-graph/episodes/{name}",
            get(debug_concept_graph_episode_detail),
        )
        .route(
            "/debug/concept-graph/relations",
            get(debug_concept_graph_relations),
        )
        .route(
            "/debug/concept-graph/queries",
            get(debug_concept_graph_queries),
        )
        .route(
            "/debug/prompts",
            get(debug_get_prompts).post(debug_update_prompts),
        )
        .route("/debug/modules/{name}/run", post(debug_run_module))
        .route("/debug/improve/trigger", post(debug_improve_trigger))
        .route("/debug/improve/proposal", post(debug_improve_proposal))
        .route("/debug/improve/review", post(debug_improve_review))
        .route("/debug/events/stream", get(debug_events_stream))
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
    let result =
        crate::application::pipeline_service::run_debug_module(&state, name, payload).await?;
    Ok(Json(result))
}

async fn debug_events(
    State(state): State<AppState>,
    Query(query): Query<DebugEventsQuery>,
) -> Result<Json<DebugEventsResponse>, (StatusCode, String)> {
    let around_mode = query
        .around_event_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());
    let limit = query.limit.unwrap_or(200).min(1000);
    let fetch_limit = if around_mode.is_some() { 5000 } else { limit };
    let mut events = state
        .event_store
        .latest(fetch_limit)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    events = events
        .into_iter()
        .filter(|event| matches_debug_event_filters(event, &query))
        .collect::<Vec<_>>();

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

    if let Some(target_event_id) = around_mode {
        let window = query.around_window.unwrap_or(20).min(200);
        let Some(index) = events
            .iter()
            .position(|event| event.event_id == target_event_id)
        else {
            events.clear();
            return Ok(Json(DebugEventsResponse { events }));
        };
        let start = index.saturating_sub(window);
        let end = (index + window + 1).min(events.len());
        events = events[start..end].to_vec();
    } else {
        events.truncate(limit);
    }

    Ok(Json(DebugEventsResponse { events }))
}

async fn debug_events_stream(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let payload = OutboundEvent {
                        kind: "event",
                        event,
                    };
                    let data = match serde_json::to_string(&payload) {
                        Ok(data) => data,
                        Err(_) => continue,
                    };
                    return Some((Ok(SseEvent::default().data(data)), rx));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn debug_concept_graph_health(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let value = state
        .activation_concept_graph
        .debug_health()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(value))
}

async fn debug_concept_graph_stats(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let value = state
        .activation_concept_graph
        .debug_counts()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(value))
}

async fn debug_concept_graph_concepts(
    State(state): State<AppState>,
    Query(query): Query<DebugConceptSearchQuery>,
) -> Result<Json<DebugConceptSearchResponse>, (StatusCode, String)> {
    let items = state
        .activation_concept_graph
        .debug_concept_search(query.q, query.limit.unwrap_or(50))
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(DebugConceptSearchResponse { items }))
}

async fn debug_concept_graph_concept_detail(
    Path(name): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let value = state
        .activation_concept_graph
        .debug_concept_detail(name)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    match value {
        Some(concept) => Ok(Json(concept)),
        None => Err((StatusCode::NOT_FOUND, "concept not found".to_string())),
    }
}

async fn debug_concept_graph_episodes(
    State(state): State<AppState>,
    Query(query): Query<DebugConceptSearchQuery>,
) -> Result<Json<DebugConceptSearchResponse>, (StatusCode, String)> {
    let items = state
        .activation_concept_graph
        .debug_episode_search(query.q, query.limit.unwrap_or(50))
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(DebugConceptSearchResponse { items }))
}

async fn debug_concept_graph_episode_detail(
    Path(name): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let value = state
        .activation_concept_graph
        .debug_episode_detail(name)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    match value {
        Some(episode) => Ok(Json(episode)),
        None => Err((StatusCode::NOT_FOUND, "episode not found".to_string())),
    }
}

async fn debug_concept_graph_relations(
    State(state): State<AppState>,
    Query(query): Query<DebugConceptSearchQuery>,
) -> Result<Json<DebugConceptSearchResponse>, (StatusCode, String)> {
    let items = state
        .activation_concept_graph
        .debug_relation_search(query.q, query.limit.unwrap_or(80))
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(DebugConceptSearchResponse { items }))
}

async fn debug_concept_graph_queries(
    State(state): State<AppState>,
    Query(query): Query<DebugConceptGraphQueriesQuery>,
) -> Result<Json<DebugConceptGraphQueriesResponse>, (StatusCode, String)> {
    let limit = query.limit.unwrap_or(100).max(1).min(500);
    let fetch_limit = (limit * 5).min(2000);
    let events = state
        .event_store
        .latest(fetch_limit)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let mut items = Vec::<DebugConceptGraphQueryItem>::new();
    for event in events {
        if !event.meta.tags.iter().any(|tag| tag == "concept_graph.query") {
            continue;
        }
        let query_terms = event
            .payload
            .get("query_terms")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let result_concepts = event
            .payload
            .get("result_concepts")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let error = event
            .payload
            .get("error")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let limit_value = event
            .payload
            .get("limit")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize);
        items.push(DebugConceptGraphQueryItem {
            event_id: event.event_id,
            ts: event.ts,
            query_terms,
            limit: limit_value,
            result_concepts,
            error,
        });
        if items.len() >= limit {
            break;
        }
    }
    Ok(Json(DebugConceptGraphQueriesResponse { items }))
}

fn matches_debug_event_filters(event: &Event, query: &DebugEventsQuery) -> bool {
    let source_ok = query
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|source| event.source.eq_ignore_ascii_case(source))
        .unwrap_or(true);
    if !source_ok {
        return false;
    }

    let module_ok = query
        .module
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|expected| {
            event_module_name(event)
                .map(|actual| actual.eq_ignore_ascii_case(expected))
                .unwrap_or(false)
        })
        .unwrap_or(true);
    if !module_ok {
        return false;
    }

    let since_ok = query
        .since_ts
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|since| event.ts.as_str() >= since)
        .unwrap_or(true);
    if !since_ok {
        return false;
    }

    query
        .until_ts
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|until| event.ts.as_str() <= until)
        .unwrap_or(true)
}

fn event_module_name(event: &Event) -> Option<&str> {
    if event.source.eq_ignore_ascii_case("decision") {
        return Some("decision");
    }
    if let Some(name) = event
        .source
        .strip_prefix("submodule:")
        .filter(|value| !value.trim().is_empty())
    {
        return Some(name);
    }

    event
        .payload
        .get("module")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            event
                .meta
                .tags
                .iter()
                .find_map(|tag| tag.strip_prefix("module:"))
                .filter(|value| !value.trim().is_empty())
        })
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

async fn debug_monitor_ui() -> Html<String> {
    const EMBEDDED: &str = include_str!("../static/monitor_ui.html");
    const MONITOR_UI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static/monitor_ui.html");
    match tokio::fs::read_to_string(MONITOR_UI_PATH).await {
        Ok(html) => Html(html),
        Err(err) => {
            println!(
                "MONITOR_UI_READ_ERROR path={} error={} (falling back to embedded html)",
                MONITOR_UI_PATH, err
            );
            Html(EMBEDDED.to_string())
        }
    }
}

async fn debug_style(Path(name): Path<String>) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (disk_path, embedded) = match name.as_str() {
        "ui-tokens.css" => (
            concat!(env!("CARGO_MANIFEST_DIR"), "/static/styles/ui-tokens.css"),
            include_str!("../static/styles/ui-tokens.css"),
        ),
        "ui-base.css" => (
            concat!(env!("CARGO_MANIFEST_DIR"), "/static/styles/ui-base.css"),
            include_str!("../static/styles/ui-base.css"),
        ),
        _ => return Err((StatusCode::NOT_FOUND, "style not found".to_string())),
    };
    let css = match tokio::fs::read_to_string(disk_path).await {
        Ok(value) => value,
        Err(err) => {
            println!(
                "DEBUG_STYLE_READ_ERROR path={} error={} (falling back to embedded css)",
                disk_path, err
            );
            embedded.to_string()
        }
    };
    Ok(([(CONTENT_TYPE, "text/css; charset=utf-8")], css))
}

async fn debug_concept_graph_ui() -> Html<String> {
    const EMBEDDED: &str = include_str!("../static/concept_graph_ui.html");
    const UI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static/concept_graph_ui.html");
    match tokio::fs::read_to_string(UI_PATH).await {
        Ok(html) => Html(html),
        Err(err) => {
            println!(
                "CONCEPT_GRAPH_UI_READ_ERROR path={} error={} (falling back to embedded html)",
                UI_PATH, err
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
