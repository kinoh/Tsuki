use crate::common::broadcast::{self, IdentifiedBroadcast};
use crate::common::chat::{ChatInput, ChatOutput, Modality, TokenUsage};
use crate::common::events::{self, Event, EventComponent};
use crate::common::messages::{self, MessageRecord, MessageRecordChat, MessageRepository};
use async_trait::async_trait;
use openai_dive::v1::api::Client;
use openai_dive::v1::resources::chat::{
    ChatCompletionParametersBuilder, ChatMessage, ChatMessageContent,
};
use std::sync::Arc;
use std::time::SystemTime;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

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
    #[error("OpenAI parameter builder error: {0}")]
    ParameterBuilder(
        #[from] openai_dive::v1::resources::chat::ChatCompletionParametersBuilderError,
    ),
    #[error("OpenAI response has no choices")]
    NoChoices,
    #[error("OpenAI response has no content")]
    EmptyContent,
    #[error("OpenAI response has unexpected message")]
    UnexpectedChatMessage,
}

pub const MAX_HISTORY_LENGTH: usize = 20;

pub enum Model {
    Echo,
    OpenAi(String),
}

fn to_event(output: &ChatOutput, usage: &Option<TokenUsage>) -> Option<Event> {
    if let Some(ref content) = output.content {
        Some(Event::AssistantMessage {
            modality: output.modality,
            message: content.clone(),
            usage: usage.clone(),
        })
    } else {
        None
    }
}

pub struct OpenAiCore {
    repository: Arc<RwLock<MessageRepository>>,
    openai: Client,
    model: Model,
    max_tokens: u32,
}

impl OpenAiCore {
    pub async fn new(
        repository: Arc<RwLock<MessageRepository>>,
        model: Model,
    ) -> Result<Self, Error> {
        let openai = Client::new_from_env();

        repository
            .write()
            .await
            .load_initial_prompt(include_str!("../prompt/initial.txt"))?;

        Ok(Self {
            repository,
            openai,
            model,
            max_tokens: 1000,
        })
    }

    async fn receive(&mut self, input_chat: ChatInput) -> Result<Option<Event>, Error> {
        let user_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            role: messages::Role::User,
            chat: MessageRecordChat::Input(input_chat.clone()),
            usage: None,
        };
        self.repository.write().await.append(user_record)?;

        let (output_chat_json, usage) = match &self.model {
            Model::OpenAi(model) => {
                let messages: Result<Vec<ChatMessage>, serde_json::Error> = self
                    .repository
                    .read()
                    .await
                    .get_latest_n(MAX_HISTORY_LENGTH, None)
                    .iter()
                    .map(|r| match r.role {
                        messages::Role::User => Ok(ChatMessage::User {
                            content: ChatMessageContent::Text(r.json_chat()?),
                            name: Some(r.user()),
                        }),
                        messages::Role::Assistant => Ok(ChatMessage::Assistant {
                            content: Some(ChatMessageContent::Text(r.json_chat()?)),
                            reasoning_content: None,
                            refusal: None,
                            name: Some(r.user()),
                            audio: None,
                            tool_calls: None,
                        }),
                        messages::Role::System => Ok(ChatMessage::Developer {
                            content: ChatMessageContent::Text(r.json_chat()?),
                            name: None,
                        }),
                    })
                    .collect();

                let parameters = ChatCompletionParametersBuilder::default()
                    .model(model)
                    .messages(messages?)
                    .max_tokens(self.max_tokens)
                    .build()?;

                let completion = self.openai.chat().create(parameters).await?;
                let usage = completion.usage;
                info!("token usage: {:?}", usage);
                let usage = usage.and_then(|u| match (u.prompt_tokens, u.completion_tokens) {
                    (Some(prompt), Some(completion)) => Some(TokenUsage { prompt, completion }),
                    _ => None,
                });

                let choice = completion.choices.get(0).ok_or(Error::NoChoices)?;

                let content = if let ChatMessage::Assistant {
                    content,
                    refusal: _,
                    name: _,
                    reasoning_content: _,
                    tool_calls: _,
                    audio: _,
                } = &choice.message
                {
                    content.as_ref().ok_or(Error::EmptyContent)?
                } else {
                    return Err(Error::UnexpectedChatMessage);
                };

                (content.to_string(), usage)
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
                (serde_json::to_string(&chat)?, None)
            }
        };

        let output_chat: ChatOutput = serde_json::from_str(&output_chat_json)?;
        let event = to_event(&output_chat, &usage);

        let assistant_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            role: messages::Role::Assistant,
            chat: MessageRecordChat::Output(output_chat),
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
