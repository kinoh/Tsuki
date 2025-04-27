use crate::common::broadcast::{self, IdentifiedBroadcast};
use crate::common::chat::{ChatInput, ChatOutput, Modality};
use crate::common::events::{self, Event, EventComponent};
use crate::common::messages::{self, MessageRecord, MessageRecordChat, MessageRepository};
use async_trait::async_trait;
use openai_dive::v1::api::Client;
use openai_dive::v1::resources::response::request::{ResponseInput, ResponseParametersBuilder};
use openai_dive::v1::resources::response::response::{OutputContent, ResponseOutput};
use std::sync::Arc;
use std::time::SystemTime;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum Error {
    #[error("system time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("repository error: {0}")]
    Repository(#[from] messages::Error),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
    #[error("OpenAI API error: {0}")]
    Api(#[from] openai_dive::v1::error::APIError),
    #[error("OpenAI response parameter builder error: {0}")]
    ParameterBuilder(
        #[from] openai_dive::v1::resources::response::request::ResponseParametersBuilderError,
    ),
    #[error("OpenAI response has multiple messages")]
    MultipleResponseMessage,
    #[error("OpenAI response has no messages")]
    NoResponseMessage,
}

pub enum Model {
    Echo,
    OpenAi(String),
}

fn to_event(output: &ChatOutput, usage: u32) -> Option<Event> {
    if let Some(ref content) = output.content {
        Some(Event::AssistantMessage {
            modality: output.modality,
            message: content.clone(),
            usage: usage,
        })
    } else {
        None
    }
}

fn get_response_text(outputs: Vec<ResponseOutput>) -> Result<String, Error> {
    let texts = outputs
        .iter()
        .filter_map(|o| match o {
            ResponseOutput::Message(message) => Some(
                message
                    .content
                    .iter()
                    .map(|c| match c {
                        OutputContent::Refusal { refusal } => {
                            warn!(message = refusal, "refusal");
                            format!("[refusal {}]", refusal)
                        }
                        OutputContent::Text {
                            text,
                            annotations: _,
                        } => text.clone(),
                    })
                    .collect::<Vec<String>>()
                    .join("\n\n"),
            ),
            other => {
                info!("additional output: {:?}", other);
                None
            }
        })
        .collect::<Vec<String>>();
    if texts.len() > 1 {
        Err(Error::MultipleResponseMessage)
    } else {
        texts.first().cloned().ok_or(Error::NoResponseMessage)
    }
}

pub struct OpenAiCore {
    repository: Arc<RwLock<MessageRepository>>,
    openai: Client,
    model: Model,
    max_tokens: u32,
    initial_prompt: String,
}

impl OpenAiCore {
    pub async fn new(
        repository: Arc<RwLock<MessageRepository>>,
        model: Model,
    ) -> Result<Self, Error> {
        let openai = Client::new_from_env();

        Ok(Self {
            repository,
            openai,
            model,
            max_tokens: 1000,
            initial_prompt: include_str!("../prompt/initial.txt").to_string(),
        })
    }

    async fn receive(&mut self, input_chat: ChatInput) -> Result<Option<Event>, Error> {
        let user_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            chat: MessageRecordChat::Input(input_chat.clone()),
            response_id: None,
            usage: 0,
        };
        self.repository.write().await.append(user_record)?;

        let (output_chat_json, response_id, usage) = match &self.model {
            Model::OpenAi(model) => {
                let previous_id = self
                    .repository
                    .read()
                    .await
                    .get_all()
                    .iter()
                    .rev()
                    .find_map(|r| r.response_id.as_ref())
                    .cloned();

                let mut parameters = ResponseParametersBuilder::default();
                parameters
                    .model(model)
                    .instructions(&self.initial_prompt)
                    .input(ResponseInput::Text(serde_json::to_string(&input_chat)?))
                    .max_output_tokens(self.max_tokens);
                if let Some(id) = previous_id {
                    parameters.previous_response_id(id);
                }

                let response = self.openai.responses().create(parameters.build()?).await?;
                let usage = response.usage;
                info!("token usage: {:?}", usage);

                let text = get_response_text(response.output)?;

                (text, Some(response.id), usage.total_tokens)
            }
            Model::Echo => {
                let chat = if input_chat.modality == Modality::Audio {
                    ChatOutput {
                        activity: None,
                        feeling: None,
                        modality: Modality::Audio,
                        content: Some(input_chat.content),
                    }
                } else if let Some((a, b)) = input_chat.content.split_once(' ') {
                    ChatOutput {
                        activity: None,
                        feeling: None,
                        modality: match a {
                            "Code" => Modality::Code,
                            "Memory" => Modality::Memory,
                            _ => Modality::Text,
                        },
                        content: Some(b.to_string()),
                    }
                } else {
                    ChatOutput {
                        activity: None,
                        feeling: None,
                        modality: Modality::Text,
                        content: Some(serde_json::to_string(&input_chat)?),
                    }
                };
                (serde_json::to_string(&chat)?, None, 0)
            }
        };

        let output_chat: ChatOutput = serde_json::from_str(&output_chat_json)?;
        let event = to_event(&output_chat, usage);

        let assistant_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            chat: MessageRecordChat::Output(output_chat),
            response_id,
            usage,
        };
        self.repository.write().await.append(assistant_record)?;

        Ok(event)
    }

    async fn run_internal(
        &mut self,
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), Error> {
        info!("start core");

        loop {
            let event = broadcast.recv().await?;
            if let Some(response) = match event {
                Event::RecognizedSpeech { user, message } => {
                    self.receive(ChatInput {
                        modality: Modality::Audio,
                        user,
                        content: message,
                    })
                    .await?
                }
                Event::SystemMessage { modality, message } => {
                    self.receive(ChatInput {
                        modality,
                        user: messages::SYSTEM_USER_NAME.to_string(),
                        content: message,
                    })
                    .await?
                }
                Event::TextMessage { user, message } => {
                    self.receive(ChatInput {
                        modality: Modality::Text,
                        user,
                        content: message,
                    })
                    .await?
                }
                Event::AssistantMessage {
                    modality: Modality::Memory,
                    message: _,
                    usage: _,
                } => Some(Event::SystemMessage {
                    modality: Modality::Text,
                    message: "memorized".to_string(),
                }),
                _ => None,
            } {
                broadcast.send(response)?;
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
