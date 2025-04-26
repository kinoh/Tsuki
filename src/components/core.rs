use crate::common::broadcast::{self, IdentifiedBroadcast};
use crate::common::chat::{ChatInput, ChatOutput, Modality, TokenUsage};
use crate::common::events::{self, Event, EventComponent};
use crate::common::messages::{self, MessageRecord, MessageRecordChat, MessageRepository};
use async_trait::async_trait;
use openai_api_rust::chat::{ChatApi, ChatBody};
use openai_api_rust::{Auth, Message, OpenAI, Role};
use std::sync::Arc;
use std::time::SystemTime;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Error, Debug)]
pub enum Error {
    #[error("OpenAI error: {0}")]
    OpenAi(String),
    #[error("system time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("repository error: {0}")]
    Repository(#[from] messages::Error),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
}

impl From<openai_api_rust::Error> for Error {
    fn from(value: openai_api_rust::Error) -> Self {
        match value {
            openai_api_rust::Error::ApiError(msg) => Error::OpenAi(format!("openai api: {}", msg)),
            openai_api_rust::Error::RequestError(msg) => {
                Error::OpenAi(format!("openai request: {}", msg))
            }
        }
    }
}

pub const MAX_HISTORY_LENGTH: usize = 20;

pub enum Model {
    Echo,
    OpenAi(String),
}

impl From<messages::Role> for Role {
    fn from(value: messages::Role) -> Self {
        match value {
            messages::Role::Assistant => Role::Assistant,
            messages::Role::System => Role::System,
            messages::Role::User => Role::User,
        }
    }
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
    openai: OpenAI,
    model: Model,
    max_tokens: i32,
}

impl OpenAiCore {
    pub async fn new(
        repository: Arc<RwLock<MessageRepository>>,
        model: Model,
    ) -> Result<Self, Error> {
        let auth = Auth::from_env().map_err(|e| Error::OpenAi(format!("auth: {}", e)))?;
        let openai = OpenAI::new(auth, "https://api.openai.com/v1/");

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
                let messages: Result<Vec<Message>, serde_json::Error> = self
                    .repository
                    .read()
                    .await
                    .get_latest_n(MAX_HISTORY_LENGTH, None)
                    .iter()
                    .map(|r| {
                        Ok(Message {
                            role: r.role.into(),
                            content: r.json_chat()?,
                        })
                    })
                    .collect();

                let chat_body = ChatBody {
                    model: model.clone(),
                    messages: messages?,
                    user: Some(input_chat.user),
                    max_tokens: Some(self.max_tokens),
                    temperature: None,
                    top_p: None,
                    n: Some(1),
                    stream: Some(false),
                    stop: None,
                    presence_penalty: None,
                    frequency_penalty: None,
                    logit_bias: None,
                };
                let completion = self.openai.chat_completion_create(&chat_body)?;

                let usage = completion.usage;
                info!(
                    prompt = usage.prompt_tokens,
                    completion = usage.completion_tokens,
                    "token usage"
                );
                let usage = match (usage.prompt_tokens, usage.completion_tokens) {
                    (Some(prompt), Some(completion)) => Some(TokenUsage { prompt, completion }),
                    _ => None,
                };

                let choice = completion
                    .choices
                    .get(0)
                    .ok_or(Error::OpenAi("no completion".to_string()))?;
                let response = choice
                    .message
                    .as_ref()
                    .ok_or(Error::OpenAi("empty completion".to_string()))?;

                (response.content.clone(), usage)
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
