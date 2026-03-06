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

impl AppState {
    pub(crate) fn new(
        services: AppServices,
        auth: AuthState,
        config: AppConfigState,
        prompts: PromptState,
        runtime: RuntimeState,
        metadata: AppMetadata,
    ) -> Self {
        Self {
            services,
            auth,
            config,
            prompts,
            runtime,
            metadata,
        }
    }
}

impl AuthState {
    pub(crate) fn new(
        web_auth_token: String,
        admin_password: String,
        admin_password_fingerprint: String,
    ) -> Self {
        Self {
            web_auth_token,
            admin_password,
            admin_password_fingerprint,
        }
    }
}

impl PromptState {
    pub(crate) fn new(
        overrides: Arc<RwLock<PromptOverrides>>,
        path: PathBuf,
        resolved: ResolvedPrompts,
    ) -> Self {
        Self {
            overrides,
            path,
            resolved,
        }
    }
}

impl ResolvedPrompts {
    pub(crate) fn new(
        base_instructions: String,
        router_instructions: String,
        decision_instructions: String,
    ) -> Self {
        Self {
            base_instructions,
            router_instructions,
            decision_instructions,
        }
    }
}

impl RuntimeState {
    pub(crate) fn new(modules: Modules, router_model: String) -> Self {
        Self {
            modules,
            router_model,
            submodule_saturation_levels: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
