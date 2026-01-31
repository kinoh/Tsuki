use async_openai::{
  config::OpenAIConfig,
  types::responses::{
    CreateResponseArgs, FunctionCallOutput, FunctionCallOutputItemParam, FunctionToolCall, InputItem,
    InputParam, Item, OutputItem, Response, Tool,
  },
  Client,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

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

pub trait ToolHandler: Send + Sync {
  fn handle(&self, tool_name: &str, arguments: &str) -> Result<String, ToolError>;
}

#[derive(Debug)]
pub struct ToolError {
  message: String,
}

impl ToolError {
  pub fn new(message: impl Into<String>) -> Self {
    Self { message: message.into() }
  }
}

impl fmt::Display for ToolError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl Error for ToolError {}

#[derive(Clone)]
pub struct ResponseApiConfig {
  pub model: String,
  pub instructions: String,
  pub temperature: Option<f32>,
  pub max_output_tokens: Option<u32>,
  pub tools: Vec<Tool>,
  pub tool_handler: Option<Arc<dyn ToolHandler>>,
  pub max_tool_rounds: usize,
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
    let mut response = self
      .create_response(InputParam::Text(request.input), None)
      .await?;

    if let Some(handler) = &self.config.tool_handler {
      for _ in 0..self.config.max_tool_rounds {
        let calls = extract_function_calls(&response.output);
        if calls.is_empty() {
          break;
        }

        let outputs = calls
          .iter()
          .map(|call| build_tool_output(call, handler.as_ref()))
          .collect::<Vec<_>>();

        let items = outputs
          .into_iter()
          .map(|output| InputItem::Item(Item::FunctionCallOutput(output)))
          .collect::<Vec<_>>();

        response = self
          .create_response(InputParam::Items(items), Some(response.id.clone()))
          .await?;
      }
    }

    let text = response
      .output_text()
      .filter(|value| !value.trim().is_empty())
      .unwrap_or_else(|| "(empty response)".to_string());
    let raw = serde_json::to_value(&response)
      .unwrap_or_else(|_| json!({ "error": "failed to serialize response" }));

    Ok(LlmResponse { text, raw })
  }
}

impl ResponseApiAdapter {
  async fn create_response(
    &self,
    input: InputParam,
    previous_response_id: Option<String>,
  ) -> Result<Response, LlmError> {
    let mut builder = CreateResponseArgs::default();
    builder
      .model(&self.config.model)
      .input(input)
      .instructions(self.config.instructions.clone());

    if let Some(temperature) = self.config.temperature {
      builder.temperature(temperature);
    }

    if let Some(max_output_tokens) = self.config.max_output_tokens {
      builder.max_output_tokens(max_output_tokens);
    }

    if !self.config.tools.is_empty() {
      builder.tools(self.config.tools.clone());
    }

    if let Some(previous) = previous_response_id {
      builder.previous_response_id(previous);
    }

    let built = builder.build().map_err(|err| LlmError::new(err.to_string()))?;
    self
      .client
      .responses()
      .create(built)
      .await
      .map_err(|err| LlmError::new(err.to_string()))
  }
}

fn extract_function_calls(items: &[OutputItem]) -> Vec<FunctionToolCall> {
  items
    .iter()
    .filter_map(|item| match item {
      OutputItem::FunctionCall(call) => Some(call.clone()),
      _ => None,
    })
    .collect()
}

fn build_tool_output(call: &FunctionToolCall, handler: &dyn ToolHandler) -> FunctionCallOutputItemParam {
  let output = match handler.handle(&call.name, &call.arguments) {
    Ok(value) => value,
    Err(err) => format!("error: {}", err),
  };
  FunctionCallOutputItemParam {
    call_id: call.call_id.clone(),
    output: FunctionCallOutput::Text(output),
    id: None,
    status: None,
  }
}
