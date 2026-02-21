use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub llm: LlmConfig,
    pub limits: LimitsConfig,
    #[serde(default)]
    pub router: RouterConfig,
    pub input: InputConfig,
    #[serde(default)]
    pub prompts: PromptsConfig,
    pub db: DbConfig,
    pub modules: Vec<ModuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub model: String,
    pub temperature: f32,
    pub temperature_enabled: bool,
    pub max_output_tokens: u32,
    pub max_tool_rounds: usize,
    pub base_personality: String,
    pub router_instructions: String,
    pub decision_instructions: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    pub decision_history: usize,
    pub submodule_history: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_concept_top_n")]
    pub concept_top_n: usize,
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
            concept_top_n: default_concept_top_n(),
            query_terms_max: default_query_terms_max(),
            hard_trigger_threshold: default_hard_trigger_threshold(),
            recommendation_threshold: default_recommendation_threshold(),
        }
    }
}

fn default_concept_top_n() -> usize {
    5
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
    #[serde(default = "default_prompts_path")]
    pub path: String,
}

impl Default for PromptsConfig {
    fn default() -> Self {
        Self {
            path: default_prompts_path(),
        }
    }
}

fn default_prompts_path() -> String {
    "data/prompts.md".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleConfig {
    pub name: String,
    pub instructions: String,
    pub enabled: bool,
}

pub fn load_config(path: &str) -> Result<Config, String> {
    let raw =
        std::fs::read_to_string(path).map_err(|err| format!("failed to read config: {}", err))?;
    toml::from_str::<Config>(&raw).map_err(|err| format!("failed to parse config: {}", err))
}
