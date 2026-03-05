use axum::{
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Extension, Path, Query, Request, State,
    },
    http::{
        header::{CONTENT_LENGTH, CONTENT_TYPE, HOST, ORIGIN, REFERER, SET_COOKIE},
        HeaderMap, Method, StatusCode,
    },
    middleware::Next,
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post, put},
    Json, Router,
};
use futures::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    convert::Infallible, net::SocketAddr, path::PathBuf, process::Command, sync::Arc,
    time::Duration, time::Instant,
};
use time::OffsetDateTime;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::activation_concept_graph::{ActivationConceptGraphStore, ConceptGraphStore};
use crate::application::module_bootstrap::{
    build_modules, sync_module_registry_from_prompts, Modules,
};
use crate::clock::now_iso8601;
use crate::config::{load_config, Config, InputConfig, LimitsConfig, RouterConfig, TtsConfig};
use crate::db::{Db, RuntimeConfigRecord};
use crate::event::Event;
use crate::event_store::EventStore;
use crate::module_registry::{ModuleRegistry, ModuleRegistryReader};
use crate::notification::FcmNotificationSender;
use crate::prompts::{load_prompts, write_prompts, PromptOverrides};
use crate::scheduler::ScheduleStore;
use crate::state::{DbStateStore, StateStore};

const ADMIN_SESSION_COOKIE_NAME: &str = "tsuki_admin_session";
const ADMIN_SESSION_ABSOLUTE_TTL_SECS: i64 = 2_592_000;
const ADMIN_SESSION_IDLE_TIMEOUT_SECS: i64 = 86_400;

#[derive(Clone, Debug)]
struct AdminSessionContext {
    session_id: String,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) db: Arc<Db>,
    pub(crate) event_store: Arc<EventStore>,
    pub(crate) tx: broadcast::Sender<Event>,
    pub(crate) auth_token: String,
    pub(crate) admin_auth_password: String,
    pub(crate) admin_password_fingerprint: String,
    pub(crate) fcm_sender: Option<FcmNotificationSender>,
    pub(crate) state_store: Arc<dyn StateStore>,
    pub(crate) activation_concept_graph: Arc<dyn ConceptGraphStore>,
    pub(crate) modules: Modules,
    pub(crate) limits: LimitsConfig,
    pub(crate) router: RouterConfig,
    pub(crate) input: InputConfig,
    pub(crate) api_versions: ApiVersions,
    pub(crate) prompts: Arc<RwLock<PromptOverrides>>,
    pub(crate) prompts_path: PathBuf,
    pub(crate) router_model: String,
    pub(crate) router_instructions: String,
    pub(crate) decision_instructions: String,
    pub(crate) tts: TtsConfig,
    pub(crate) submodule_saturation_levels: Arc<RwLock<std::collections::HashMap<String, f64>>>,
}

#[derive(Debug, Serialize)]
struct OutboundEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    event: Event,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugRunRequest {
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
    #[serde(default)]
    router: Option<String>,
    decision: String,
    #[serde(default)]
    self_improvement: Option<String>,
    submodules: Vec<PromptModulePayload>,
}

#[derive(Debug, Deserialize)]
struct AuthLoginRequest {
    password: String,
}

#[derive(Debug, Serialize)]
struct AuthLoginResponse {
    ok: bool,
}

#[derive(Debug, Serialize)]
struct AuthMeResponse {
    authenticated: bool,
}

#[derive(Debug, Serialize)]
struct AuthLogoutResponse {
    ok: bool,
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
struct EventsQuery {
    limit: Option<usize>,
    before_ts: Option<String>,
    order: Option<String>,
    tags: Option<String>,
    #[serde(rename = "tags[]")]
    tags_bracket: Option<String>,
}

#[derive(Debug, Serialize)]
struct EventsResponse {
    items: Vec<Event>,
}

#[derive(Debug, Serialize)]
struct RuntimeConfigPayload {
    #[serde(rename = "enableNotification")]
    enable_notification: bool,
    #[serde(rename = "enableSensory")]
    enable_sensory: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ApiVersions {
    asyncapi: Option<String>,
    openapi: Option<String>,
}

#[derive(Debug, Serialize)]
struct MetadataApiVersions {
    asyncapi: Option<String>,
    openapi: Option<String>,
}

#[derive(Debug, Serialize)]
struct MetadataResponse {
    git_hash: Option<String>,
    openai_model: String,
    mcp_tools: Vec<String>,
    api_versions: MetadataApiVersions,
    router_model: String,
    active_modules: Vec<String>,
}

#[derive(Debug, Serialize)]
struct NotificationTokensResponse {
    tokens: Vec<String>,
}

#[derive(Debug, Serialize)]
struct NotificationResult {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct JaAccentResponse {
    accent: String,
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
    query_text: Option<String>,
    limit: Option<usize>,
    result_concepts: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct DebugConceptGraphQueriesResponse {
    items: Vec<DebugConceptGraphQueryItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugTriggerRequest {
    pub(crate) event: String,
    #[serde(default)]
    pub(crate) payload: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugTriggerResponse {
    pub(crate) event_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugImproveProposalRequest {
    pub(crate) target: String,
    pub(crate) job_id: String,
    pub(crate) diff_text: String,
    #[serde(default)]
    pub(crate) requires_approval: Option<bool>,
    #[serde(default)]
    pub(crate) created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugImproveReviewRequest {
    pub(crate) proposal_id: String,
    pub(crate) job_id: String,
    pub(crate) target: String,
    pub(crate) decision: String,
    #[serde(default)]
    pub(crate) reviewed_by: Option<String>,
    #[serde(default)]
    pub(crate) review_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugImproveResponse {
    pub(crate) proposal_id: Option<String>,
    pub(crate) review_event_id: Option<String>,
    pub(crate) apply_event_id: Option<String>,
    pub(crate) applied: bool,
}

pub(crate) async fn run_server() {
    let config = load_config("config.toml").expect("failed to load config");
    validate_required_config(&config);
    let port = config.server.port;
    let auth_token = std::env::var("WEB_AUTH_TOKEN").expect("WEB_AUTH_TOKEN is required");
    let admin_auth_password =
        std::env::var("ADMIN_AUTH_PASSWORD").expect("ADMIN_AUTH_PASSWORD is required");
    let admin_password_fingerprint = admin_password_fingerprint(&admin_auth_password);
    let (tx, _) = broadcast::channel(256);
    let db = Db::connect(&config.db).await.expect("failed to init db");
    db.delete_admin_sessions_not_matching_fingerprint(&admin_password_fingerprint)
        .await
        .expect("failed to invalidate outdated admin sessions");
    let event_store = Arc::new(EventStore::new(db.clone()));
    let schedule_store = Arc::new(ScheduleStore::new(db.clone()));
    let prompts_path = prompts_path_from_config(&config);
    let prompt_overrides = load_prompts(&prompts_path).unwrap_or_else(|err| {
        panic!(
            "failed to load prompts '{}': {}",
            prompts_path.display(),
            err
        )
    });
    let base_instructions = required_prompt(
        prompt_overrides.base.as_deref(),
        "Base",
        prompts_path.as_path(),
    );
    let router_instructions = required_prompt(
        prompt_overrides.router.as_deref(),
        "Router",
        prompts_path.as_path(),
    );
    let decision_instructions = required_prompt(
        prompt_overrides.decision.as_deref(),
        "Decision",
        prompts_path.as_path(),
    );
    required_prompt(
        prompt_overrides.self_improvement.as_deref(),
        "Self Improvement",
        prompts_path.as_path(),
    );
    let prompts = Arc::new(RwLock::new(prompt_overrides.clone()));
    let emit_event = crate::application::event_service::build_emit_event_callback(
        event_store.clone(),
        tx.clone(),
    );
    let state_store: Arc<dyn StateStore> = Arc::new(DbStateStore::new(db.clone()));
    let activation_concept_graph = Arc::new(
        ActivationConceptGraphStore::connect(
            config.concept_graph.memgraph_uri.clone(),
            config.concept_graph.memgraph_user.clone(),
            std::env::var("MEMGRAPH_PASSWORD").unwrap_or_default(),
            config.concept_graph.arousal_tau_ms,
        )
        .await
        .expect("failed to connect activation concept graph store"),
    );
    let module_registry = ModuleRegistry::new(db.clone());
    sync_module_registry_from_prompts(&module_registry, &prompt_overrides)
        .await
        .expect("failed to sync module registry from prompts.md");
    let modules = build_modules(
        state_store.clone(),
        activation_concept_graph.clone(),
        schedule_store.clone(),
        module_registry,
        &config,
        base_instructions,
        emit_event,
    );

    let fcm_sender = match FcmNotificationSender::from_env() {
        Ok(sender) => Some(sender),
        Err(err) => {
            eprintln!("FCM_SENDER_INIT_ERROR error={}", err);
            None
        }
    };

    let state = AppState {
        db: db.clone(),
        event_store,
        tx,
        auth_token,
        admin_auth_password,
        admin_password_fingerprint,
        fcm_sender,
        state_store,
        activation_concept_graph,
        modules,
        limits: config.limits.clone(),
        router: config.router.clone(),
        input: config.input.clone(),
        api_versions: load_api_versions(),
        prompts,
        prompts_path,
        router_model: config
            .llm
            .router_model
            .clone()
            .unwrap_or_else(|| config.llm.model.clone()),
        router_instructions,
        decision_instructions,
        tts: config.tts.clone(),
        submodule_saturation_levels: Arc::new(RwLock::new(std::collections::HashMap::new())),
    };
    crate::application::improve_service::start_trigger_consumer(state.clone());
    crate::application::scheduler_service::start_scheduler(
        state.clone(),
        schedule_store.clone(),
        config.scheduler.clone(),
    )
    .await
    .expect("failed to start scheduler");
    crate::application::scheduler_notice_service::start_notice_consumer(state.clone());

    let admin_router = Router::new()
        .route("/styles/{name}", get(debug_style))
        .route("/prompts", get(debug_ui))
        .route("/events", get(debug_monitor_ui))
        .route("/concept-graph", get(debug_concept_graph_ui))
        .route("/concept-graph/health", get(debug_concept_graph_health))
        .route("/concept-graph/stats", get(debug_concept_graph_stats))
        .route("/concept-graph/concepts", get(debug_concept_graph_concepts))
        .route(
            "/concept-graph/concepts/{name}",
            get(debug_concept_graph_concept_detail),
        )
        .route("/concept-graph/episodes", get(debug_concept_graph_episodes))
        .route(
            "/concept-graph/episodes/{name}",
            get(debug_concept_graph_episode_detail),
        )
        .route(
            "/concept-graph/relations",
            get(debug_concept_graph_relations),
        )
        .route("/concept-graph/queries", get(debug_concept_graph_queries))
        .route(
            "/prompts/data",
            get(debug_get_prompts).post(debug_update_prompts),
        )
        .route("/modules/{name}/run", post(debug_run_module))
        .route("/events/stream", get(debug_events_stream))
        .route("/events/list", get(debug_events))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ));

    let auth_router = Router::new()
        .route("/me", get(auth_me))
        .route("/logout", post(auth_logout))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ));

    let app = Router::new()
        .route("/", get(ws_handler))
        .route("/events", get(events))
        .route("/metadata", get(metadata_get))
        .route("/config", get(config_get).put(config_put))
        .route(
            "/notification/token",
            put(notification_token_put).delete(notification_token_delete),
        )
        .route("/notification/tokens", get(notification_tokens_get))
        .route("/notification/_test", post(notification_test))
        .route("/tts", post(tts_post))
        .route("/auth/login", post(auth_login))
        .route("/admin/login", get(auth_login_page))
        .route("/triggers", post(improve_trigger))
        .route("/proposals", post(improve_proposal))
        .route("/reviews", post(improve_review))
        .nest("/auth", auth_router)
        .nest("/admin", admin_router)
        .layer(axum::middleware::from_fn(access_log_middleware))
        .with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!("rust core ws listening on ws://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind listener");
    axum::serve(listener, app).await.expect("server error");
}

fn validate_required_config(config: &Config) {
    if config.prompts.path.trim().is_empty() {
        panic!("config.toml [prompts].path must not be empty");
    }
    if config.concept_graph.memgraph_uri.trim().is_empty() {
        panic!("config.toml [concept_graph].memgraph_uri must not be empty");
    }
    if config.concept_graph.arousal_tau_ms <= 0.0 {
        panic!("config.toml [concept_graph].arousal_tau_ms must be > 0");
    }
    if config.tts.ja_accent_url.trim().is_empty() {
        panic!("config.toml [tts].ja_accent_url must not be empty");
    }
    if config.tts.voicevox_url.trim().is_empty() {
        panic!("config.toml [tts].voicevox_url must not be empty");
    }
    if config.tts.voicevox_speaker == 0 {
        panic!("config.toml [tts].voicevox_speaker must be > 0");
    }
    if config.tts.voicevox_timeout_ms == 0 {
        panic!("config.toml [tts].voicevox_timeout_ms must be > 0");
    }
}

fn prompts_path_from_config(config: &Config) -> PathBuf {
    let trimmed = config.prompts.path.trim();
    if trimmed.is_empty() {
        panic!("config.toml [prompts].path must not be empty");
    }
    PathBuf::from(trimmed)
}

fn required_prompt(raw: Option<&str>, section: &str, prompts_path: &std::path::Path) -> String {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            panic!(
                "prompts file '{}' requires non-empty '{}' section",
                prompts_path.display(),
                section
            )
        })
}

async fn access_log_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());
    let remote_ip = extract_remote_ip(request.headers());
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("-")
        .to_string();
    let req_bytes = content_length_from_headers(request.headers());
    let ts = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "-".to_string());
    let started = Instant::now();
    let response = next.run(request).await;
    let res_bytes = content_length_from_headers(response.headers());
    println!(
        "HTTP_ACCESS ts={} remote_ip={} method={} path={} status={} req_bytes={} res_bytes={} ua=\"{}\" elapsed_ms={}",
        ts,
        remote_ip,
        method,
        path,
        response.status().as_u16(),
        req_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string()),
        res_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string()),
        sanitize_log_field(&user_agent),
        started.elapsed().as_millis()
    );
    response
}

fn extract_remote_ip(headers: &HeaderMap) -> String {
    if let Some(value) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if let Some(first) = value
            .split(',')
            .map(str::trim)
            .find(|part| !part.is_empty())
            .map(str::to_string)
        {
            return first;
        }
    }
    headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "-".to_string())
}

fn content_length_from_headers(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn sanitize_log_field(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
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

async fn auth_login(
    State(state): State<AppState>,
    Json(payload): Json<AuthLoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if payload.password != state.admin_auth_password {
        println!("ADMIN_AUTH_LOGIN_FAILURE reason=invalid_password");
        return Err((StatusCode::UNAUTHORIZED, "invalid credentials".to_string()));
    }

    let session_id = Uuid::new_v4().to_string();
    let now = now_iso8601();
    state
        .db
        .create_admin_session(
            &session_id,
            now.as_str(),
            now.as_str(),
            &state.admin_password_fingerprint,
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    println!("ADMIN_AUTH_LOGIN_SUCCESS");
    Ok((
        [(SET_COOKIE, build_admin_session_cookie(&session_id))],
        Json(AuthLoginResponse { ok: true }),
    ))
}

async fn auth_login_page() -> Html<String> {
    const EMBEDDED: &str = include_str!("../static/admin_login.html");
    const LOGIN_UI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static/admin_login.html");
    match tokio::fs::read_to_string(LOGIN_UI_PATH).await {
        Ok(html) => Html(html),
        Err(err) => {
            println!(
                "ADMIN_LOGIN_UI_READ_ERROR path={} error={} (falling back to embedded html)",
                LOGIN_UI_PATH, err
            );
            Html(EMBEDDED.to_string())
        }
    }
}

async fn auth_me() -> Json<AuthMeResponse> {
    Json(AuthMeResponse {
        authenticated: true,
    })
}

async fn auth_logout(
    State(state): State<AppState>,
    Extension(session): Extension<AdminSessionContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    state
        .db
        .delete_admin_session(&session.session_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    println!("ADMIN_AUTH_LOGOUT");
    Ok((
        [(SET_COOKIE, build_admin_session_clear_cookie())],
        Json(AuthLogoutResponse { ok: true }),
    ))
}

async fn admin_auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    if method_requires_csrf(request.method()) {
        validate_admin_csrf(&request)?;
    }
    let session_id = extract_cookie_value(request.headers(), ADMIN_SESSION_COOKIE_NAME)
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "admin session required".to_string(),
            )
        })?;
    let session = state
        .db
        .get_admin_session(&session_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "invalid admin session".to_string(),
            )
        })?;
    let now = OffsetDateTime::now_utc();

    if session.password_fingerprint != state.admin_password_fingerprint {
        state
            .db
            .delete_admin_session(&session.session_id)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        println!("ADMIN_AUTH_SESSION_CLEANUP reason=password_changed");
        return Err((
            StatusCode::UNAUTHORIZED,
            "invalid admin session".to_string(),
        ));
    }

    let created_at = match parse_rfc3339_to_utc(session.created_at.as_str()) {
        Ok(value) => value,
        Err(_) => {
            state
                .db
                .delete_admin_session(&session.session_id)
                .await
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            println!("ADMIN_AUTH_SESSION_CLEANUP reason=invalid_created_at");
            return Err((
                StatusCode::UNAUTHORIZED,
                "invalid admin session".to_string(),
            ));
        }
    };
    let last_seen_at = match parse_rfc3339_to_utc(session.last_seen_at.as_str()) {
        Ok(value) => value,
        Err(_) => {
            state
                .db
                .delete_admin_session(&session.session_id)
                .await
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
            println!("ADMIN_AUTH_SESSION_CLEANUP reason=invalid_last_seen_at");
            return Err((
                StatusCode::UNAUTHORIZED,
                "invalid admin session".to_string(),
            ));
        }
    };
    let expired_absolute =
        now.unix_timestamp() - created_at.unix_timestamp() > ADMIN_SESSION_ABSOLUTE_TTL_SECS;
    let expired_idle =
        now.unix_timestamp() - last_seen_at.unix_timestamp() > ADMIN_SESSION_IDLE_TIMEOUT_SECS;
    if expired_absolute || expired_idle {
        state
            .db
            .delete_admin_session(&session.session_id)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        println!(
            "ADMIN_AUTH_SESSION_CLEANUP reason={}",
            if expired_absolute {
                "absolute_ttl_expired"
            } else {
                "idle_timeout_expired"
            }
        );
        return Err((
            StatusCode::UNAUTHORIZED,
            "admin session expired".to_string(),
        ));
    }

    state
        .db
        .update_admin_session_last_seen(&session.session_id, now_iso8601().as_str())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    request.extensions_mut().insert(AdminSessionContext {
        session_id: session.session_id.clone(),
    });
    Ok(next.run(request).await)
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
    let current_overrides = state.prompts.read().await.clone();
    let overrides = PromptOverrides {
        base: Some(payload.base.clone()),
        router: payload
            .router
            .clone()
            .or_else(|| current_overrides.router.clone()),
        decision: Some(payload.decision.clone()),
        self_improvement: payload
            .self_improvement
            .clone()
            .or_else(|| current_overrides.self_improvement.clone()),
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

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Json<EventsResponse>, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;

    let limit = query.limit.unwrap_or(50);
    if limit == 0 || limit > 500 {
        return Err((StatusCode::BAD_REQUEST, "invalid limit".to_string()));
    }

    if let Some(before_ts) = query.before_ts.as_deref() {
        validate_iso8601(before_ts)
            .map_err(|message| (StatusCode::BAD_REQUEST, message.to_string()))?;
    }

    let desc = match query
        .order
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => true,
        Some(value) if value.eq_ignore_ascii_case("desc") => true,
        Some(value) if value.eq_ignore_ascii_case("asc") => false,
        Some(_) => return Err((StatusCode::BAD_REQUEST, "invalid order".to_string())),
    };

    let tags = parse_events_query_tags(&query);
    let items = if tags.is_empty() {
        state
            .event_store
            .list(limit, query.before_ts.as_deref(), desc)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
    } else {
        list_events_with_tags(
            &state,
            limit,
            query.before_ts.as_deref(),
            desc,
            tags.as_slice(),
        )
        .await?
    };

    Ok(Json(EventsResponse { items }))
}

async fn list_events_with_tags(
    state: &AppState,
    limit: usize,
    before_ts: Option<&str>,
    desc: bool,
    tags: &[String],
) -> Result<Vec<Event>, (StatusCode, String)> {
    let mut items = Vec::with_capacity(limit);
    let mut cursor = before_ts.map(str::to_string);
    let batch_size = limit.saturating_mul(4).clamp(50, 500);
    let mut scanned = 0usize;
    let max_scanned = 5_000usize;

    while items.len() < limit && scanned < max_scanned {
        let batch = state
            .event_store
            .list(batch_size, cursor.as_deref(), desc)
            .await
            .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
        if batch.is_empty() {
            break;
        }

        scanned += batch.len();
        cursor = batch.last().map(|event| event.ts.clone());

        for event in batch {
            if event_has_any_tag(&event, tags) {
                items.push(event);
                if items.len() >= limit {
                    break;
                }
            }
        }

        if scanned >= max_scanned {
            break;
        }
    }

    Ok(items)
}

async fn config_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RuntimeConfigPayload>, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;
    let config = state
        .db
        .get_runtime_config()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(runtime_config_payload(config)))
}

async fn metadata_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MetadataResponse>, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;
    let active_modules = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .into_iter()
        .map(|module| module.name)
        .collect::<Vec<_>>();

    Ok(Json(MetadataResponse {
        git_hash: get_git_hash(),
        openai_model: state.modules.runtime.model.clone(),
        mcp_tools: Vec::new(),
        api_versions: MetadataApiVersions {
            asyncapi: state.api_versions.asyncapi.clone(),
            openapi: state.api_versions.openapi.clone(),
        },
        router_model: state.router_model.clone(),
        active_modules,
    }))
}

async fn config_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<RuntimeConfigPayload>, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;
    let (enable_notification, enable_sensory) = parse_runtime_config_payload(&payload)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid payload".to_string()))?;
    let config = state
        .db
        .set_runtime_config(enable_notification, enable_sensory)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(runtime_config_payload(config)))
}

async fn notification_token_put(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<NotificationResult>, (StatusCode, String)> {
    let user = parse_http_auth_user(&headers, &state.auth_token)?;
    let token = parse_notification_token_payload(&payload).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "missing token parameter".to_string(),
        )
    })?;
    state
        .db
        .add_notification_token(&user, &token)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(NotificationResult { ok: true }))
}

async fn notification_token_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<NotificationResult>, (StatusCode, String)> {
    let user = parse_http_auth_user(&headers, &state.auth_token)?;
    let token = parse_notification_token_payload(&payload).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "missing token parameter".to_string(),
        )
    })?;
    state
        .db
        .remove_notification_token(&user, &token)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(NotificationResult { ok: true }))
}

async fn notification_tokens_get(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<NotificationTokensResponse>, (StatusCode, String)> {
    let user = parse_http_auth_user(&headers, &state.auth_token)?;
    let tokens = state
        .db
        .list_notification_tokens(&user)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(Json(NotificationTokensResponse { tokens }))
}

async fn notification_test(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<NotificationResult>, (StatusCode, String)> {
    let user = parse_http_auth_user(&headers, &state.auth_token)?;
    let config = state
        .db
        .get_runtime_config()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if !config.enable_notification {
        return Err((
            StatusCode::CONFLICT,
            "notifications are disabled".to_string(),
        ));
    }

    let sender = state.fcm_sender.clone().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "notification sender is not configured".to_string(),
        )
    })?;
    let tokens = state
        .db
        .list_notification_tokens(&user)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    sender
        .send_to_tokens(&tokens, "Test Notification", "This is a test notification.")
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;

    Ok(Json(NotificationResult { ok: true }))
}

async fn tts_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;

    let message = parse_tts_message(&payload)?;
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Message is required".to_string()));
    }

    let speaker = state.tts.voicevox_speaker;
    let timeout = Duration::from_millis(state.tts.voicevox_timeout_ms);
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(internal_error)?;

    let accent_base = state.tts.ja_accent_url.clone();
    let accent_url = format!("{}/accent", accent_base.trim_end_matches('/'));
    let accent_response = client
        .post(accent_url)
        .json(&serde_json::json!({ "text": message }))
        .send()
        .await
        .map_err(map_tts_transport_error)?;
    let accent: JaAccentResponse = accent_response.json().await.map_err(internal_error)?;
    println!(
        "TTS_ACCENT_GENERATED message={} accent={}",
        message, accent.accent
    );

    let voicevox_base = state.tts.voicevox_url.clone();
    let phrases_url = format!("{}/accent_phrases", voicevox_base.trim_end_matches('/'));
    let phrases_response = client
        .post(phrases_url)
        .query(&[
            ("speaker", speaker.to_string()),
            ("text", accent.accent),
            ("is_kana", "true".to_string()),
        ])
        .send()
        .await
        .map_err(map_tts_transport_error)?;

    if !phrases_response.status().is_success() {
        let status = phrases_response.status().as_u16();
        let body = phrases_response.text().await.unwrap_or_default();
        println!(
            "TTS_VOICEVOX_ACCENT_PHRASES_FAILED status={} body={}",
            status, body
        );
        return Err((
            StatusCode::BAD_GATEWAY,
            "VoiceVox accent_phrases failed".to_string(),
        ));
    }

    let accent_phrases: Value = phrases_response.json().await.map_err(internal_error)?;
    let query = serde_json::json!({
        "accent_phrases": accent_phrases,
        "speedScale": 1.15,
        "pitchScale": -0.02,
        "intonationScale": 1.4,
        "volumeScale": 1.0,
        "pauseLengthScale": 0.4,
        "prePhonemeLength": 0,
        "postPhonemeLength": 0,
        "outputSamplingRate": 24000,
        "outputStereo": false
    });

    let synthesis_url = format!("{}/synthesis", voicevox_base.trim_end_matches('/'));
    let synth_response = client
        .post(synthesis_url)
        .query(&[("speaker", speaker.to_string())])
        .json(&query)
        .send()
        .await
        .map_err(map_tts_transport_error)?;

    if !synth_response.status().is_success() {
        let status = synth_response.status().as_u16();
        let body = synth_response.text().await.unwrap_or_default();
        println!(
            "TTS_VOICEVOX_SYNTHESIS_FAILED status={} body={}",
            status, body
        );
        return Err((
            StatusCode::BAD_GATEWAY,
            "VoiceVox synthesis failed".to_string(),
        ));
    }

    let content_length = synth_response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let audio = synth_response.bytes().await.map_err(internal_error)?;

    let mut response = Response::new(axum::body::Body::from(audio));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        CONTENT_TYPE,
        "audio/wav"
            .parse()
            .expect("audio/wav is a valid header value"),
    );
    if let Some(length) = content_length {
        if let Ok(value) = length.parse() {
            response.headers_mut().insert(CONTENT_LENGTH, value);
        }
    }

    Ok(response)
}

fn parse_tts_message(payload: &Value) -> Result<&str, (StatusCode, String)> {
    let object = payload
        .as_object()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Invalid payload".to_string()))?;
    let message = object
        .get("message")
        .and_then(|value| value.as_str())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Invalid payload".to_string()))?;
    Ok(message.trim())
}

async fn debug_events_stream(
    State(state): State<AppState>,
) -> Result<Sse<impl futures::Stream<Item = Result<SseEvent, Infallible>>>, (StatusCode, String)> {
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
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
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
        if !event
            .meta
            .tags
            .iter()
            .any(|tag| tag == "concept_graph.query")
        {
            continue;
        }
        let query_text = event
            .payload
            .get("query_text")
            .and_then(|value| value.as_str())
            .map(str::to_string);
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
            query_text,
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

fn normalize_event_tags(raw_tags: &[&str]) -> Vec<String> {
    raw_tags
        .iter()
        .map(|tag| tag.trim().to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

fn parse_events_query_tags(query: &EventsQuery) -> Vec<String> {
    let source = query
        .tags
        .as_deref()
        .or(query.tags_bracket.as_deref())
        .unwrap_or("");
    let values = source
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalize_event_tags(values.as_slice())
}

fn event_has_any_tag(event: &Event, tags: &[String]) -> bool {
    event
        .meta
        .tags
        .iter()
        .any(|event_tag| tags.iter().any(|tag| event_tag.eq_ignore_ascii_case(tag)))
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

async fn improve_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DebugTriggerRequest>,
) -> Result<Json<DebugTriggerResponse>, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;
    let result =
        crate::application::trigger_ingress_api::trigger_improvement(&state, payload).await?;
    Ok(Json(result))
}

async fn improve_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DebugImproveProposalRequest>,
) -> Result<Json<DebugImproveResponse>, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;
    let result =
        crate::application::improve_approval_service::propose_improvement(&state, payload).await?;
    Ok(Json(result))
}

async fn improve_review(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DebugImproveReviewRequest>,
) -> Result<Json<DebugImproveResponse>, (StatusCode, String)> {
    verify_http_auth(&headers, &state.auth_token)?;
    let result =
        crate::application::improve_approval_service::review_improvement(&state, payload).await?;
    Ok(Json(result))
}

async fn debug_ui(State(_state): State<AppState>) -> Result<Html<String>, (StatusCode, String)> {
    const EMBEDDED: &str = include_str!("../static/debug_ui.html");
    const DEBUG_UI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static/debug_ui.html");
    match tokio::fs::read_to_string(DEBUG_UI_PATH).await {
        Ok(html) => Ok(Html(html)),
        Err(err) => {
            println!(
                "DEBUG_UI_READ_ERROR path={} error={} (falling back to embedded html)",
                DEBUG_UI_PATH, err
            );
            Ok(Html(EMBEDDED.to_string()))
        }
    }
}

async fn debug_monitor_ui(
    State(_state): State<AppState>,
) -> Result<Html<String>, (StatusCode, String)> {
    const EMBEDDED: &str = include_str!("../static/monitor_ui.html");
    const MONITOR_UI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static/monitor_ui.html");
    match tokio::fs::read_to_string(MONITOR_UI_PATH).await {
        Ok(html) => Ok(Html(html)),
        Err(err) => {
            println!(
                "MONITOR_UI_READ_ERROR path={} error={} (falling back to embedded html)",
                MONITOR_UI_PATH, err
            );
            Ok(Html(EMBEDDED.to_string()))
        }
    }
}

async fn debug_style(
    Path(name): Path<String>,
    State(_state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
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

async fn debug_concept_graph_ui(
    State(_state): State<AppState>,
) -> Result<Html<String>, (StatusCode, String)> {
    const EMBEDDED: &str = include_str!("../static/concept_graph_ui.html");
    const UI_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static/concept_graph_ui.html");
    match tokio::fs::read_to_string(UI_PATH).await {
        Ok(html) => Ok(Html(html)),
        Err(err) => {
            println!(
                "CONCEPT_GRAPH_UI_READ_ERROR path={} error={} (falling back to embedded html)",
                UI_PATH, err
            );
            Ok(Html(EMBEDDED.to_string()))
        }
    }
}

fn admin_password_fingerprint(password: &str) -> String {
    let mut hash: u64 = 1469598103934665603;
    for byte in password.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{:016x}", hash)
}

fn build_admin_session_cookie(session_id: &str) -> String {
    format!(
        "{}={}; Max-Age={}; Path=/; Secure; HttpOnly; SameSite=Strict",
        ADMIN_SESSION_COOKIE_NAME, session_id, ADMIN_SESSION_ABSOLUTE_TTL_SECS
    )
}

fn build_admin_session_clear_cookie() -> String {
    format!(
        "{}=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Strict",
        ADMIN_SESSION_COOKIE_NAME
    )
}

fn method_requires_csrf(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn map_tts_transport_error(err: reqwest::Error) -> (StatusCode, String) {
    if err.is_timeout() {
        println!("TTS_REQUEST_TIMEOUT error={}", err);
        return (
            StatusCode::GATEWAY_TIMEOUT,
            "TTS request timed out".to_string(),
        );
    }
    internal_error(err)
}

fn internal_error(err: impl std::fmt::Display) -> (StatusCode, String) {
    println!("HTTP_INTERNAL_ERROR error={}", err);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error".to_string(),
    )
}

fn validate_admin_csrf(request: &Request) -> Result<(), (StatusCode, String)> {
    let expected_origin = expected_request_origin(request.headers())?;
    if let Some(origin) = request
        .headers()
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if origin == expected_origin {
            return Ok(());
        }
        return Err((StatusCode::FORBIDDEN, "csrf validation failed".to_string()));
    }

    if let Some(referer) = request
        .headers()
        .get(REFERER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if referer == expected_origin || referer.starts_with(&(expected_origin + "/")) {
            return Ok(());
        }
    }
    Err((StatusCode::FORBIDDEN, "csrf validation failed".to_string()))
}

fn expected_request_origin(headers: &HeaderMap) -> Result<String, (StatusCode, String)> {
    let host = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| (StatusCode::FORBIDDEN, "missing host header".to_string()))?;
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    Ok(format!("{}://{}", proto, host))
}

fn extract_cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let raw = headers.get("cookie")?.to_str().ok()?;
    for part in raw.split(';') {
        let mut tokens = part.trim().splitn(2, '=');
        let name = tokens.next()?.trim();
        let value = tokens.next()?.trim();
        if name == cookie_name && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn parse_rfc3339_to_utc(value: &str) -> Result<OffsetDateTime, (StatusCode, String)> {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            "invalid admin session".to_string(),
        )
    })
}

fn verify_auth(message: &str, expected_token: &str) -> bool {
    let mut parts = message.splitn(2, ':');
    let user = parts.next().unwrap_or("");
    let token = parts.next().unwrap_or("");
    !user.is_empty() && token == expected_token
}

fn verify_http_auth(headers: &HeaderMap, expected_token: &str) -> Result<(), (StatusCode, String)> {
    let _ = parse_http_auth_user(headers, expected_token)?;
    Ok(())
}

fn parse_http_auth_user(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<String, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "authorization header required".to_string(),
            )
        })?;

    let mut parts = auth.splitn(2, ':');
    let user = parts.next().unwrap_or("").trim();
    let token = parts.next().unwrap_or("").trim();

    if user.is_empty() || token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            "invalid authorization format".to_string(),
        ));
    }

    if token != expected_token {
        return Err((StatusCode::UNAUTHORIZED, "invalid token".to_string()));
    }

    Ok(user.to_string())
}

fn validate_iso8601(value: &str) -> Result<(), &'static str> {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map(|_| ())
        .map_err(|_| "invalid before_ts")
}

fn runtime_config_payload(config: RuntimeConfigRecord) -> RuntimeConfigPayload {
    let _ = config.updated_at;
    RuntimeConfigPayload {
        enable_notification: config.enable_notification,
        enable_sensory: config.enable_sensory,
    }
}

fn parse_runtime_config_payload(payload: &Value) -> Option<(bool, bool)> {
    let object = payload.as_object()?;
    let enable_notification = object.get("enableNotification")?.as_bool()?;
    let enable_sensory = object.get("enableSensory")?.as_bool()?;
    Some((enable_notification, enable_sensory))
}

fn parse_notification_token_payload(payload: &Value) -> Option<String> {
    let object = payload.as_object()?;
    let token = object.get("token")?.as_str()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

fn get_git_hash() -> Option<String> {
    if let Ok(value) = std::env::var("GIT_HASH") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn load_api_versions() -> ApiVersions {
    ApiVersions {
        asyncapi: read_spec_info_version(include_str!("../../api-specs/asyncapi.yaml")),
        openapi: read_spec_info_version(include_str!("../../api-specs/openapi.yaml")),
    }
}

fn read_spec_info_version(raw: &str) -> Option<String> {
    let Ok(doc) = serde_yaml::from_str::<SpecInfoDoc>(raw) else {
        return None;
    };
    let version = doc.info.version.trim();
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct SpecInfoDoc {
    info: SpecInfo,
}

#[derive(Debug, Deserialize)]
struct SpecInfo {
    version: String,
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
    let router = overrides
        .router
        .clone()
        .unwrap_or_else(|| state.router_instructions.clone());
    let self_improvement = overrides.self_improvement.clone().unwrap_or_default();
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
        router: Some(router),
        decision,
        self_improvement: Some(self_improvement),
        submodules,
    })
}

#[cfg(test)]
mod tests {
    use super::{event_has_any_tag, normalize_event_tags, read_spec_info_version, verify_auth};
    use crate::event::contracts::response_text;

    #[test]
    fn verify_auth_accepts_valid_user_and_token() {
        assert!(verify_auth("tester:test-token", "test-token"));
    }

    #[test]
    fn verify_auth_rejects_invalid_token() {
        assert!(!verify_auth("tester:bad-token", "test-token"));
    }

    #[test]
    fn verify_auth_rejects_missing_user() {
        assert!(!verify_auth(":test-token", "test-token"));
    }

    #[test]
    fn read_spec_info_version_parses_api_specs() {
        let asyncapi_version =
            read_spec_info_version(include_str!("../../api-specs/asyncapi.yaml"));
        let openapi_version = read_spec_info_version(include_str!("../../api-specs/openapi.yaml"));
        assert!(asyncapi_version.is_some());
        assert!(openapi_version.is_some());
    }

    #[test]
    fn normalize_event_tags_removes_empty_and_deduplicates() {
        let tags = normalize_event_tags(&[" response ", "", "input", "RESPONSE"]);
        assert_eq!(tags.len(), 2);
        assert!(tags.iter().any(|tag| tag == "response"));
        assert!(tags.iter().any(|tag| tag == "input"));
    }

    #[test]
    fn event_has_any_tag_matches_ignore_case() {
        let mut event = response_text("hello".to_string());
        event.meta.tags.push("Decision".to_string());
        assert!(event_has_any_tag(&event, &["response".to_string()]));
        assert!(event_has_any_tag(&event, &["decision".to_string()]));
        assert!(!event_has_any_tag(&event, &["input".to_string()]));
    }
}
