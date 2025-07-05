use age::ssh::Identity;
use anyhow::{Context, Result};
use async_trait::async_trait;
use openai_dive::v1::api::Client;
use openai_dive::v1::resources::response::items::{FunctionToolCallOutput, InputItemStatus};
use openai_dive::v1::resources::response::request::{
    ContentInput, InputItem, InputMessage, ResponseInput, ResponseInputItem,
    ResponseParametersBuilder,
};
use openai_dive::v1::resources::response::response::{
    OutputContent, OutputMessage, ResponseOutput, Role,
};
use openai_dive::v1::resources::response::shared::{ResponseTool, ResponseToolChoice};
use serde_json::json;
use std::io::BufReader;
use tera::{Context as TeraContext, Tera};
use tracing::{debug, info, warn};

use crate::common::chat::{
    ChatInput, ChatInputFunctionCall, ChatOutput, ChatOutputFunctionCall, ChatOutputMessage,
};

pub const TEMPLATE_NAME: &str = "instruction";

fn decrypt_prompt(private_key: &str) -> Result<String> {
    let encrypted = include_bytes!("../prompt/initial.txt.age");

    let buf = BufReader::new(private_key.as_bytes());
    let identity = Identity::from_buffer(buf, None)?;
    let decrypted = age::decrypt(&identity, encrypted)?;

    Ok(str::from_utf8(&decrypted)?.to_string())
}

#[async_trait]
pub trait Function {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn args_schema(&self) -> serde_json::Value;
    async fn call(&self, args_json: &str) -> Result<String, String>;
}

pub struct Thinker {
    client: Client,
    functions: Vec<Box<dyn Function + Send + Sync>>,
    initial_prompt: Tera,
}

impl Thinker {
    pub fn new(prompt_key: &str, api_key: &str) -> Result<Self> {
        let client = Client::new(api_key.to_string());

        let mut initial_prompt = Tera::default();
        initial_prompt.add_raw_template(TEMPLATE_NAME, &decrypt_prompt(prompt_key)?)?;

        Ok(Self {
            client,
            initial_prompt,
            functions: vec![],
        })
    }

    pub fn register_function<T: Function + Send + Sync + 'static>(&mut self, function: T) {
        self.functions.push(Box::new(function));
    }

    pub async fn think(
        &self,
        model: &String,
        memories: Vec<String>,
        max_tokens: u32,
        input_chats: Vec<ChatInput>,
        previous_id: Option<&String>,
    ) -> Result<(Vec<ChatOutput>, String, u32)> {
        let tools: Vec<ResponseTool> = self
            .functions
            .iter()
            .map(|f| ResponseTool::Function {
                name: f.name().to_string(),
                description: Some(f.description().to_string()),
                parameters: f.args_schema(),
                strict: true,
            })
            .collect();

        let inputs = input_chats
            .iter()
            .map(|c| {
                Ok(match c {
                    ChatInput::Message(message) => ResponseInputItem::Message(InputMessage {
                        role: Role::User,
                        content: ContentInput::Text(serde_json::to_string(message)?),
                    }),
                    ChatInput::FunctionCall(call) => ResponseInputItem::Item(
                        InputItem::FunctionToolCallOutput(FunctionToolCallOutput {
                            id: None,
                            call_id: call.call_id.clone(),
                            output: call.output.clone(),
                            status: InputItemStatus::Completed,
                        }),
                    ),
                })
            })
            .collect::<Result<Vec<ResponseInputItem>, anyhow::Error>>()?;

        let context = TeraContext::from_value(json!({
            "memories": memories,
        }))?;

        debug!("responses API inputs: {}", serde_json::to_string(&inputs)?);

        let mut parameters = ResponseParametersBuilder::default();
        parameters
            .model(model)
            .instructions(self.initial_prompt.render(TEMPLATE_NAME, &context)?)
            .input(ResponseInput::List(inputs))
            .tools(tools)
            .tool_choice(ResponseToolChoice::Auto)
            .max_output_tokens(max_tokens);
        if let Some(id) = previous_id {
            parameters.previous_response_id(id);
        }
        let parameters = parameters.build()?;

        let response = self.client.responses().create(parameters).await?;
        let usage = response.usage;
        info!("token usage: {:?}", &usage);

        let output_chats = response
            .output
            .iter()
            .map(|r| {
                Ok(match r {
                    ResponseOutput::Message(message) => {
                        ChatOutput::Message(parse_message(&message)?)
                    }
                    ResponseOutput::FunctionToolCall(call) => {
                        ChatOutput::FunctionCall(ChatOutputFunctionCall {
                            call_id: call.call_id.clone(),
                            name: call.name.clone(),
                            args: call.arguments.clone(),
                        })
                    }
                    output => ChatOutput::BuiltinToolCall(serde_json::to_value(output)?),
                })
            })
            .collect::<Result<Vec<ChatOutput>, anyhow::Error>>()?;

        Ok((output_chats, response.id, usage.total_tokens))
    }

    pub async fn do_call(&self, call: &ChatOutputFunctionCall) -> ChatInputFunctionCall {
        info!(name = &call.name, "function call");

        let output = if let Some(func) = self.functions.iter().find(|f| f.name() == call.name) {
            let output = func
                .call(&call.args)
                .await
                .unwrap_or_else(|e| format!("error: {}", e));
            info!(output = &output, "function call finished");
            output
        } else {
            warn!("unknown function");
            "error: unknown function".to_string()
        };

        ChatInputFunctionCall {
            call_id: call.call_id.clone(),
            output,
        }
    }
}

fn parse_message(message: &OutputMessage) -> Result<ChatOutputMessage, anyhow::Error> {
    let content = message
        .content
        .first()
        .context("OpenAI response message has no content")?;

    match *content {
        OutputContent::Refusal { ref refusal } => {
            warn!(message = refusal, "refusal");
            anyhow::bail!("OpenAI response message has refusal");
        }
        OutputContent::Text {
            ref text,
            annotations: _,
        } => Ok(serde_json::from_str(&text)?),
    }
}
