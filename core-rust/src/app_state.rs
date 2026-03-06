use crate::activation_concept_graph::ConceptGraphStore;
use crate::application::module_bootstrap::Modules;
use crate::config::{InputConfig, LimitsConfig, RouterConfig, TtsConfig};
use crate::db::Db;
use crate::event::Event;
use crate::event_store::EventStore;
use crate::mcp::McpRegistry;
use crate::notification::FcmNotificationSender;
use crate::prompts::PromptOverrides;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone)]
pub(crate) struct ApiVersions {
    pub(crate) asyncapi: Option<String>,
    pub(crate) openapi: Option<String>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) services: AppServices,
    pub(crate) auth: AuthState,
    pub(crate) config: AppConfigState,
    pub(crate) prompts: PromptState,
    pub(crate) runtime: RuntimeState,
    pub(crate) metadata: AppMetadata,
}

#[derive(Clone)]
pub(crate) struct AppServices {
    pub(crate) db: Arc<Db>,
    pub(crate) event_store: Arc<EventStore>,
    pub(crate) tx: broadcast::Sender<Event>,
    pub(crate) fcm_sender: Option<FcmNotificationSender>,
    pub(crate) activation_concept_graph: Arc<dyn ConceptGraphStore>,
    pub(crate) mcp_registry: Arc<McpRegistry>,
}

#[derive(Clone)]
pub(crate) struct AuthState {
    pub(crate) web_auth_token: String,
    pub(crate) admin_password: String,
    pub(crate) admin_password_fingerprint: String,
}

#[derive(Clone)]
pub(crate) struct AppConfigState {
    pub(crate) limits: LimitsConfig,
    pub(crate) router: RouterConfig,
    pub(crate) input: InputConfig,
    pub(crate) tts: TtsConfig,
}

#[derive(Clone)]
pub(crate) struct PromptState {
    pub(crate) overrides: Arc<RwLock<PromptOverrides>>,
    pub(crate) path: PathBuf,
    pub(crate) resolved: ResolvedPrompts,
}

#[derive(Clone)]
pub(crate) struct ResolvedPrompts {
    pub(crate) base_instructions: String,
    pub(crate) router_instructions: String,
    pub(crate) decision_instructions: String,
}

#[derive(Clone)]
pub(crate) struct RuntimeState {
    pub(crate) modules: Modules,
    pub(crate) router_model: String,
    pub(crate) submodule_saturation_levels: Arc<RwLock<HashMap<String, f64>>>,
}

#[derive(Clone)]
pub(crate) struct AppMetadata {
    pub(crate) api_versions: ApiVersions,
    pub(crate) mcp_available_tools: Arc<Vec<String>>,
}
