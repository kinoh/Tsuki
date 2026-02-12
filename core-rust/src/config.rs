use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub llm: LlmConfig,
    pub limits: LimitsConfig,
    #[serde(default)]
    pub router: RouterConfig,
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
    #[serde(default = "default_recommendation_threshold")]
    pub recommendation_threshold: f32,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            concept_top_n: default_concept_top_n(),
            recommendation_threshold: default_recommendation_threshold(),
        }
    }
}

fn default_concept_top_n() -> usize {
    5
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
