use async_openai::{
    config::OpenAIConfig,
    types::responses::{
        CreateResponseArgs, FunctionCallOutput, FunctionCallOutputItemParam, FunctionToolCall,
        InputItem, InputParam, Item, OutputItem, Response, Tool,
    },
    Client,
};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub input: String,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub text: String,
    pub raw: Value,
    pub tool_calls: Vec<ToolCallTrace>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallTrace {
    pub call_id: String,
    pub name: String,
    pub output: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmUsage {
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
}

#[derive(Debug)]
pub struct LlmError {
    message: String,
}

impl LlmError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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
pub struct LlmUsageContext {
    pub user_id: String,
    pub agent_name: String,
}

impl LlmUsageContext {
    pub fn new(user_id: impl Into<String>, agent_name: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            agent_name: agent_name.into(),
        }
    }
}

#[async_trait]
pub trait LlmUsageRecorder: Send + Sync {
    async fn record_usage(
        &self,
        response_id: &str,
        usage: &LlmUsage,
        context: &LlmUsageContext,
    ) -> Result<(), String>;
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
        Self {
            message: message.into(),
        }
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
    pub usage_recorder: Option<Arc<dyn LlmUsageRecorder>>,
    pub usage_context: Option<LlmUsageContext>,
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
        let respond_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let total_started = Instant::now();
        println!(
            "PERF llm respond_id={} stage=start model={} input_chars={} max_tool_rounds={} tools={}",
            respond_id,
            self.config.model,
            request.input.len(),
            self.config.max_tool_rounds,
            self.config.tools.len()
        );
        let mut tool_calls = Vec::<ToolCallTrace>::new();
        let mut llm_round_count = 0usize;
        let initial_started = Instant::now();
        let mut response = match self
            .create_response(InputParam::Text(request.input), None)
            .await
        {
            Ok(value) => {
                llm_round_count += 1;
                println!(
                    "PERF llm respond_id={} stage=llm_call round={} ms={} ok=true",
                    respond_id,
                    llm_round_count,
                    initial_started.elapsed().as_millis()
                );
                value
            }
            Err(err) => {
                println!(
                    "PERF llm respond_id={} stage=llm_call round={} ms={} ok=false error={}",
                    respond_id,
                    llm_round_count + 1,
                    initial_started.elapsed().as_millis(),
                    err
                );
                return Err(err);
            }
        };

        if let Some(handler) = &self.config.tool_handler {
            for round in 0..self.config.max_tool_rounds {
                let calls = extract_function_calls(&response.output);
                if calls.is_empty() {
                    println!(
                        "PERF llm respond_id={} stage=tool_round round={} calls=0 stop=true",
                        respond_id,
                        round + 1
                    );
                    break;
                }
                println!(
                    "PERF llm respond_id={} stage=tool_round round={} calls={}",
                    respond_id,
                    round + 1,
                    calls.len()
                );

                let mut outputs_with_trace = Vec::with_capacity(calls.len());
                for call in &calls {
                    let tool_started = Instant::now();
                    let (output, trace) = build_tool_output_with_trace(call, handler.as_ref());
                    println!(
                        "PERF llm respond_id={} stage=tool_call round={} name={} ms={} error={}",
                        respond_id,
                        round + 1,
                        trace.name,
                        tool_started.elapsed().as_millis(),
                        trace.error.is_some()
                    );
                    outputs_with_trace.push((output, trace));
                }
                tool_calls.extend(outputs_with_trace.iter().map(|(_, trace)| trace.clone()));

                let items = outputs_with_trace
                    .into_iter()
                    .map(|(output, _)| InputItem::Item(Item::FunctionCallOutput(output)))
                    .collect::<Vec<_>>();

                let followup_started = Instant::now();
                response = match self
                    .create_response(InputParam::Items(items), Some(response.id.clone()))
                    .await
                {
                    Ok(value) => {
                        llm_round_count += 1;
                        println!(
                            "PERF llm respond_id={} stage=llm_call round={} ms={} ok=true",
                            respond_id,
                            llm_round_count,
                            followup_started.elapsed().as_millis()
                        );
                        value
                    }
                    Err(err) => {
                        println!(
                            "PERF llm respond_id={} stage=llm_call round={} ms={} ok=false error={}",
                            respond_id,
                            llm_round_count + 1,
                            followup_started.elapsed().as_millis(),
                            err
                        );
                        return Err(err);
                    }
                };
            }
        }

        let text = response
            .output_text()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "(empty response)".to_string());
        let raw = serde_json::to_value(&response)
            .unwrap_or_else(|_| json!({ "error": "failed to serialize response" }));
        let usage = extract_usage_from_raw(&raw);
        let response_id = response.id.clone();
        if let (Some(recorder), Some(context), Some(usage_value)) = (
            &self.config.usage_recorder,
            &self.config.usage_context,
            &usage,
        ) {
            if let Err(err) = recorder
                .record_usage(&response_id, usage_value, context)
                .await
            {
                eprintln!(
                    "LLM_USAGE_RECORD_ERROR user_id={} agent_name={} response_id={} error={}",
                    context.user_id, context.agent_name, response_id, err
                );
            }
        }
        println!(
            "PERF llm respond_id={} stage=end total_ms={} output_chars={} tool_calls={} llm_rounds={}",
            respond_id,
            total_started.elapsed().as_millis(),
            text.len(),
            tool_calls.len(),
            llm_round_count
        );

        Ok(LlmResponse {
            text,
            raw,
            tool_calls,
        })
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

        let built = builder
            .build()
            .map_err(|err| LlmError::new(err.to_string()))?;
        self.client
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

fn build_tool_output_with_trace(
    call: &FunctionToolCall,
    handler: &dyn ToolHandler,
) -> (FunctionCallOutputItemParam, ToolCallTrace) {
    let (output, error) = match handler.handle(&call.name, &call.arguments) {
        Ok(value) => (value, None),
        Err(err) => {
            let message = err.to_string();
            (format!("error: {}", message), Some(message))
        }
    };
    let item = FunctionCallOutputItemParam {
        call_id: call.call_id.clone(),
        output: FunctionCallOutput::Text(output.clone()),
        id: None,
        status: None,
    };
    let trace = ToolCallTrace {
        call_id: call.call_id.clone(),
        name: call.name.clone(),
        output,
        error,
    };
    (item, trace)
}

fn extract_usage_from_raw(raw: &Value) -> Option<LlmUsage> {
    let usage = raw.get("usage")?.as_object()?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|value| value.as_i64())
        .or_else(|| usage.get("prompt_tokens").and_then(|value| value.as_i64()));
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|value| value.as_i64())
        .or_else(|| {
            usage
                .get("completion_tokens")
                .and_then(|value| value.as_i64())
        });
    let total_tokens = usage.get("total_tokens").and_then(|value| value.as_i64());
    let reasoning_tokens = usage
        .get("output_tokens_details")
        .and_then(|value| value.as_object())
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(|value| value.as_i64());
    let cached_input_tokens = usage
        .get("input_tokens_details")
        .and_then(|value| value.as_object())
        .and_then(|details| details.get("cached_tokens"))
        .and_then(|value| value.as_i64());
    if input_tokens.is_none()
        && output_tokens.is_none()
        && total_tokens.is_none()
        && reasoning_tokens.is_none()
        && cached_input_tokens.is_none()
    {
        return None;
    }
    Some(LlmUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        reasoning_tokens,
        cached_input_tokens,
    })
}
