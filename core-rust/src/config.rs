use crate::scheduler::{ScheduleAction, ScheduleRecurrence};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub llm: LlmConfig,
    pub limits: LimitsConfig,
    #[serde(default)]
    pub router: RouterConfig,
    pub input: InputConfig,
    pub db: DbConfig,
    #[serde(default)]
    pub prompts: Option<PromptsConfig>,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub model: String,
    #[serde(default)]
    pub router_model: Option<String>,
    pub temperature: f32,
    pub temperature_enabled: bool,
    pub max_output_tokens: u32,
    pub max_tool_rounds: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    pub decision_history: usize,
    pub submodule_history: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_query_terms_max")]
    pub query_terms_max: usize,
    #[serde(default = "default_hard_trigger_threshold")]
    pub hard_trigger_threshold: f32,
    #[serde(default = "default_recommendation_threshold")]
    pub recommendation_threshold: f32,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            query_terms_max: default_query_terms_max(),
            hard_trigger_threshold: default_hard_trigger_threshold(),
            recommendation_threshold: default_recommendation_threshold(),
        }
    }
}

fn default_query_terms_max() -> usize {
    8
}

fn default_hard_trigger_threshold() -> f32 {
    0.85
}

fn default_recommendation_threshold() -> f32 {
    0.6
}

#[derive(Debug, Clone, Deserialize)]
pub struct DbConfig {
    pub path: String,
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputConfig {
    pub router_context_template: String,
    pub decision_context_template: String,
    pub submodule_context_template: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptsConfig {
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_scheduler_tick_interval_ms")]
    pub tick_interval_ms: u64,
    #[serde(default)]
    pub self_improvement: Option<SchedulerSelfImprovementConfig>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tick_interval_ms: default_scheduler_tick_interval_ms(),
            self_improvement: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerSelfImprovementConfig {
    #[serde(default = "default_scheduler_policy_enabled")]
    pub enabled: bool,
    pub timezone: String,
    pub recurrence: ScheduleRecurrence,
    pub action: ScheduleAction,
}

fn default_scheduler_tick_interval_ms() -> u64 {
    1000
}

fn default_scheduler_policy_enabled() -> bool {
    true
}

pub fn load_config(path: &str) -> Result<Config, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|err| format!("failed to read config: {}", err))?;
    toml::from_str::<Config>(&raw).map_err(|err| format!("failed to parse config: {}", err))
}
