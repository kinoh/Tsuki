use crate::scheduler::{ScheduleAction, ScheduleRecurrence};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub llm: LlmConfig,
    pub limits: LimitsConfig,
    #[serde(default)]
    pub router: RouterConfig,
    #[serde(default)]
    pub conversation_recall: ConversationRecallConfig,
    pub input: InputConfig,
    pub db: DbConfig,
    pub prompts: PromptsConfig,
    pub concept_graph: ConceptGraphConfig,
    pub tts: TtsConfig,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
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
    #[serde(default = "default_active_state_limit")]
    pub active_state_limit: usize,
    #[serde(default = "default_hard_trigger_threshold")]
    pub hard_trigger_threshold: f32,
    #[serde(default = "default_recommendation_threshold")]
    pub recommendation_threshold: f32,
    #[serde(default)]
    pub multimodal_embedding: RouterMultimodalEmbeddingConfig,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            query_terms_max: default_query_terms_max(),
            active_state_limit: default_active_state_limit(),
            hard_trigger_threshold: default_hard_trigger_threshold(),
            recommendation_threshold: default_recommendation_threshold(),
            multimodal_embedding: RouterMultimodalEmbeddingConfig::default(),
        }
    }
}

fn default_query_terms_max() -> usize {
    8
}

fn default_active_state_limit() -> usize {
    8
}

fn default_hard_trigger_threshold() -> f32 {
    0.85
}

fn default_recommendation_threshold() -> f32 {
    0.6
}

fn default_multimodal_embedding_model() -> String {
    "gemini-embedding-2-preview".to_string()
}

fn default_multimodal_primary_source() -> String {
    "text".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouterMultimodalEmbeddingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub shadow_enabled: bool,
    #[serde(default = "default_multimodal_primary_source")]
    pub primary_source: String,
    #[serde(default = "default_multimodal_embedding_model")]
    pub model: String,
    #[serde(default)]
    pub output_dimensionality: usize,
}

impl Default for RouterMultimodalEmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            shadow_enabled: false,
            primary_source: default_multimodal_primary_source(),
            model: default_multimodal_embedding_model(),
            output_dimensionality: 0,
        }
    }
}

fn default_conversation_recall_enabled() -> bool {
    true
}

fn default_conversation_recall_limit() -> usize {
    5
}

fn default_conversation_recall_surrounding_event_window() -> usize {
    5
}

fn default_conversation_recall_semantic_weight() -> f64 {
    0.85
}

fn default_conversation_recall_recency_weight() -> f64 {
    0.15
}

fn default_conversation_recall_recency_tau_ms() -> f64 {
    1000.0 * 60.0 * 60.0 * 24.0 * 30.0
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
pub struct ConversationRecallConfig {
    #[serde(default = "default_conversation_recall_enabled")]
    pub enabled: bool,
    #[serde(default = "default_conversation_recall_limit")]
    pub top_k_hits: usize,
    #[serde(default = "default_conversation_recall_surrounding_event_window")]
    pub surrounding_event_window: usize,
    #[serde(default = "default_conversation_recall_semantic_weight")]
    pub semantic_weight: f64,
    #[serde(default = "default_conversation_recall_recency_weight")]
    pub recency_weight: f64,
    #[serde(default = "default_conversation_recall_recency_tau_ms")]
    pub recency_tau_ms: f64,
}

impl Default for ConversationRecallConfig {
    fn default() -> Self {
        Self {
            enabled: default_conversation_recall_enabled(),
            top_k_hits: default_conversation_recall_limit(),
            surrounding_event_window: default_conversation_recall_surrounding_event_window(),
            semantic_weight: default_conversation_recall_semantic_weight(),
            recency_weight: default_conversation_recall_recency_weight(),
            recency_tau_ms: default_conversation_recall_recency_tau_ms(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptsConfig {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConceptGraphConfig {
    pub memgraph_uri: String,
    #[serde(default)]
    pub memgraph_user: String,
    pub arousal_tau_ms: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    pub ja_accent_url: String,
    pub voicevox_url: String,
    pub voicevox_speaker: u32,
    pub voicevox_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    pub url: String,
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
