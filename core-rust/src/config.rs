use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
  pub server: ServerConfig,
  pub llm: LlmConfig,
  pub limits: LimitsConfig,
  pub db: DbConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
  pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
  pub model: String,
  pub temperature: f32,
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
pub struct DbConfig {
  pub path: String,
  pub remote_url: Option<String>,
}

pub fn load_config(path: &str) -> Result<Config, String> {
  let raw = std::fs::read_to_string(path)
    .map_err(|err| format!("failed to read config: {}", err))?;
  toml::from_str::<Config>(&raw).map_err(|err| format!("failed to parse config: {}", err))
}
