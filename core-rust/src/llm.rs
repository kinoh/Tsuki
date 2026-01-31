use async_openai::{
  config::OpenAIConfig,
  types::responses::CreateResponseArgs,
  Client,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone)]
pub struct LlmRequest {
  pub input: String,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
  pub text: String,
  pub raw: Value,
}

#[derive(Debug)]
pub struct LlmError {
  message: String,
}

impl LlmError {
  pub fn new(message: impl Into<String>) -> Self {
    Self { message: message.into() }
  }
}

impl fmt::Display for LlmError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl Error for LlmError {}

#[async_trait]
pub trait LlmAdapter: Send + Sync {
  async fn respond(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
}

#[derive(Debug, Clone)]
pub struct ResponseApiConfig {
  pub model: String,
  pub instructions: String,
  pub temperature: Option<f32>,
  pub max_output_tokens: Option<u32>,
}

pub struct ResponseApiAdapter {
  client: Client<OpenAIConfig>,
  config: ResponseApiConfig,
}

impl ResponseApiAdapter {
  pub fn new(config: ResponseApiConfig) -> Self {
    Self {
      client: Client::new(),
      config,
    }
  }
}

#[async_trait]
impl LlmAdapter for ResponseApiAdapter {
  async fn respond(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
    let mut builder = CreateResponseArgs::default();
    builder
      .model(&self.config.model)
      .input(request.input)
      .instructions(self.config.instructions.clone());

    if let Some(temperature) = self.config.temperature {
      builder.temperature(temperature);
    }

    if let Some(max_output_tokens) = self.config.max_output_tokens {
      builder.max_output_tokens(max_output_tokens);
    }

    let built = builder.build().map_err(|err| LlmError::new(err.to_string()))?;
    let response = self
      .client
      .responses()
      .create(built)
      .await
      .map_err(|err| LlmError::new(err.to_string()))?;

    let text = response
      .output_text()
      .filter(|value| !value.trim().is_empty())
      .unwrap_or_else(|| "(empty response)".to_string());
    let raw = serde_json::to_value(&response)
      .unwrap_or_else(|_| json!({ "error": "failed to serialize response" }));

    Ok(LlmResponse { text, raw })
  }
}
