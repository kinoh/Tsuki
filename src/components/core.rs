use crate::common::broadcast::{self, IdentifiedBroadcast};
use crate::common::chat::{
    ChatInput, ChatInputFunctionCall, ChatInputMessage, ChatOutput, ChatOutputFunctionCall,
    ChatOutputMessage, Modality,
};
use crate::common::events::{self, Event, EventComponent};
use crate::common::memory::MemoryRecord;
use crate::common::message::{self, MessageRecord, MessageRecordChat};
use crate::common::repository::{self, Repository};
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
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::SystemTime;
use tera::{Context, Tera};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum Error {
    #[error("system time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("repository error: {0}")]
    Repository(#[from] repository::Error),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
    #[error("OpenAI API error: {0}")]
    Api(#[from] openai_dive::v1::error::APIError),
    #[error("OpenAI response parameter builder error: {0}")]
    ParameterBuilder(
        #[from] openai_dive::v1::resources::response::request::ResponseParametersBuilderError,
    ),
    #[error("Tera error: {0}")]
    Tera(#[from] tera::Error),
    #[error("OpenAI response message has no content")]
    NoMessageContent,
    #[error("OpenAI response message has refusal")]
    Refusal,
    #[error("OpenAI response called unknown function")]
    UnknownFunctionCall,
}

pub const TEMPLATE_NAME: &str = "instruction";

pub enum Model {
    Echo,
    OpenAi(String),
}

fn to_event(message: &ChatOutputMessage, usage: u32) -> Option<Event> {
    if let Some(ref content) = message.content {
        Some(Event::AssistantMessage {
            modality: message.modality,
            message: content.clone(),
            usage: usage,
        })
    } else {
        None
    }
}

fn parse_message(message: &OutputMessage) -> Result<ChatOutputMessage, Error> {
    let content = message.content.first().ok_or(Error::NoMessageContent)?;

    match *content {
        OutputContent::Refusal { ref refusal } => {
            warn!(message = refusal, "refusal");
            Err(Error::Refusal)
        }
        OutputContent::Text {
            ref text,
            annotations: _,
        } => Ok(serde_json::from_str(&text)?),
    }
}

fn get_timestamp() -> Result<u64, Error> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs())
}

#[derive(Deserialize)]
struct MemorizeFunctionArguments {
    items: Vec<String>,
}

pub struct OpenAiCore {
    repository: Arc<RwLock<Repository>>,
    openai: Client,
    model: Model,
    max_tokens: u32,
    initial_prompt: Tera,
}

impl OpenAiCore {
    pub async fn new(repository: Arc<RwLock<Repository>>, model: Model) -> Result<Self, Error> {
        let openai = Client::new_from_env();
        let mut initial_prompt = Tera::default();
        initial_prompt.add_raw_template(TEMPLATE_NAME, include_str!("../prompt/initial.txt"))?;

        Ok(Self {
            repository,
            openai,
            model,
            max_tokens: 1000,
            initial_prompt,
        })
    }

    async fn think_openai(
        &self,
        model: &String,
        input_chats: Vec<ChatInput>,
        previous_id: Option<&String>,
    ) -> Result<(Vec<ChatOutput>, String, u32), Error> {
        let tools = vec![ResponseTool::Function {
            name: "memorize".to_string(),
            description: Some("Save knowledge to memories section in instruction".to_string()),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "description": "ultimately short summary of memory; stored to memories section in instruction"
                        }
                    }
                },
                "required": ["items"],
                "additionalProperties": false
            }),
            strict: true,
        }];

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
            .collect::<Result<Vec<ResponseInputItem>, Error>>()?;

        let memories = self
            .repository
            .read()
            .await
            .memories()
            .iter()
            .flat_map(|r| r.content.clone())
            .collect::<Vec<String>>();
        let context = Context::from_value(json!({
            "memories": memories,
        }))?;

        let mut parameters = ResponseParametersBuilder::default();
        parameters
            .model(model)
            .instructions(self.initial_prompt.render(TEMPLATE_NAME, &context)?)
            .input(ResponseInput::List(inputs))
            .tools(tools)
            .tool_choice(ResponseToolChoice::Auto)
            .max_output_tokens(self.max_tokens);
        if let Some(id) = previous_id {
            parameters.previous_response_id(id);
        }
        let parameters = parameters.build()?;
        debug!(
            "responses API parameters: {}",
            serde_json::to_string(&parameters)?
        );

        let response = self.openai.responses().create(parameters).await?;
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
            .collect::<Result<Vec<ChatOutput>, Error>>()?;

        Ok((output_chats, response.id, usage.total_tokens))
    }

    fn think_echo(
        &self,
        input_chats: Vec<ChatInput>,
    ) -> Result<(Vec<ChatOutput>, String, u32), Error> {
        let chat = if let ChatInput::Message(ref message) = input_chats[0] {
            if message.modality == Modality::Audio {
                ChatOutputMessage {
                    activity: None,
                    feeling: None,
                    modality: Modality::Audio,
                    content: Some(message.content.clone()),
                }
            } else {
                ChatOutputMessage {
                    activity: None,
                    feeling: None,
                    modality: Modality::Text,
                    content: Some(serde_json::to_string(&message)?),
                }
            }
        } else {
            ChatOutputMessage {
                activity: None,
                feeling: None,
                modality: Modality::Text,
                content: Some("no message".to_string()),
            }
        };
        Ok((vec![ChatOutput::Message(chat)], "".to_string(), 0))
    }

    async fn think_and_save(
        &mut self,
        input_chats: Vec<ChatInput>,
    ) -> Result<(Vec<ChatOutput>, u32), Error> {
        let user_record = MessageRecord {
            timestamp: get_timestamp()?,
            chat: MessageRecordChat::Input(input_chats.clone()),
            response_id: None,
            usage: 0,
        };
        self.repository.write().await.append_message(user_record)?;

        let previous_id = self.repository.read().await.last_message_id().cloned();
        let (outputs, response_id, usage) = match &self.model {
            Model::OpenAi(model) => {
                self.think_openai(&model, input_chats, previous_id.as_ref())
                    .await
            }
            Model::Echo => self.think_echo(input_chats),
        }?;

        let assistant_record = MessageRecord {
            timestamp: get_timestamp()?,
            chat: MessageRecordChat::Output(outputs.clone()),
            response_id: Some(response_id),
            usage,
        };
        self.repository
            .write()
            .await
            .append_message(assistant_record)?;

        Ok((outputs, usage))
    }

    async fn do_call(&self, call: &ChatOutputFunctionCall) -> Result<ChatInputFunctionCall, Error> {
        info!(name = &call.name, "function call");

        let output = match &*call.name {
            "memorize" => {
                let args: MemorizeFunctionArguments = serde_json::from_str(&call.args)?;
                self.repository.write().await.append_memory(MemoryRecord {
                    content: args.items,
                    timestamp: get_timestamp()?,
                })?;
                Ok("success".to_string())
            }
            _ => Err(Error::UnknownFunctionCall),
        }?;

        info!(output = &output, "function call finished");

        Ok(ChatInputFunctionCall {
            call_id: call.call_id.clone(),
            output,
        })
    }

    async fn receive(
        &mut self,
        broadcast: &IdentifiedBroadcast<Event>,
        message: ChatInputMessage,
    ) -> Result<(), Error> {
        let mut inputs = vec![ChatInput::Message(message)];
        loop {
            let (outputs, usage) = self.think_and_save(inputs).await?;
            let mut call_outputs = Vec::new();
            for output in outputs {
                match output {
                    ChatOutput::FunctionCall(call) => {
                        call_outputs.push(ChatInput::FunctionCall(self.do_call(&call).await?))
                    }
                    ChatOutput::Message(message) => {
                        if let Some(event) = to_event(&message, usage) {
                            broadcast.send(event)?;
                        }
                    }
                    ChatOutput::BuiltinToolCall(_) => {}
                }
            }
            if call_outputs.is_empty() {
                break Ok(());
            }
            inputs = call_outputs;
        }
    }

    async fn run_internal(
        &mut self,
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), Error> {
        info!("start core");

        loop {
            let event = broadcast.recv().await?;
            match event {
                Event::RecognizedSpeech { user, message } => {
                    self.receive(
                        &broadcast,
                        ChatInputMessage {
                            modality: Modality::Audio,
                            user,
                            content: message,
                        },
                    )
                    .await?
                }
                Event::SystemMessage { modality, message } => {
                    self.receive(
                        &broadcast,
                        ChatInputMessage {
                            modality,
                            user: message::SYSTEM_USER_NAME.to_string(),
                            content: message,
                        },
                    )
                    .await?
                }
                Event::TextMessage { user, message } => {
                    self.receive(
                        &broadcast,
                        ChatInputMessage {
                            modality: Modality::Text,
                            user,
                            content: message,
                        },
                    )
                    .await?
                }
                _ => (),
            }
        }
    }
}

#[async_trait]
impl EventComponent for OpenAiCore {
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| events::Error::Component(format!("core: {}", e)))
    }
}
