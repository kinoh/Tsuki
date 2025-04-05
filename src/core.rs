use crate::events::{self, Event, EventComponent};
use crate::messages::{self, MessageRecord, MessageRepository, Modality};
use async_trait::async_trait;
use openai_api_rust::chat::{ChatApi, ChatBody};
use openai_api_rust::{Auth, Message, OpenAI, Role};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;
use thiserror::Error;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::sync::RwLock;

#[derive(Error, Debug)]
pub enum Error {
    #[error("OpenAI error: {0}")]
    OpenAi(String),
    #[error("system time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("repository error: {0}")]
    Repository(#[from] crate::messages::Error),
    #[error("failed to receive event: {0}")]
    ReceiveEvent(#[from] broadcast::error::RecvError),
    #[error("failed to send event: {0}")]
    SendEvent(#[from] broadcast::error::SendError<Event>),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
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

pub const ASSISTANT_NAME: &str = "つき";

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

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAiChatInput {
    modality: Modality,
    user: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct OpenAiChatOutput {
    feeling: Option<u8>,
    activity: Option<u8>,
    modality: Modality,
    content: String,
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
            .load_initial_prompt(include_str!("prompt/initial.txt"))?;

        Ok(Self {
            repository,
            openai,
            model,
            max_tokens: 1000,
        })
    }

    async fn receive(&mut self, input_chat: OpenAiChatInput) -> Result<OpenAiChatOutput, Error> {
        let input_chat_json = serde_json::to_string(&input_chat)?;
        let user_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            modality: input_chat.modality,
            role: messages::Role::User,
            user: input_chat.user.clone(),
            chat: input_chat_json,
        };
        self.repository.write().await.append(user_record)?;

        let output_chat_json = match &self.model {
            Model::OpenAi(model) => {
                let records = self.repository.read().await.get_all().to_vec();
                let messages: Vec<Message> = records
                    .iter()
                    .map(|r| Message {
                        role: r.role.into(),
                        content: r.chat.clone(),
                    })
                    .collect();

                let chat_body = ChatBody {
                    model: model.clone(),
                    messages,
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

                let choice = completion
                    .choices
                    .get(0)
                    .ok_or(Error::OpenAi("no completion".to_string()))?;
                let response = choice
                    .message
                    .as_ref()
                    .ok_or(Error::OpenAi("empty completion".to_string()))?;

                response.content.clone()
            }
            Model::Echo => {
                let chat = OpenAiChatOutput {
                    activity: None,
                    feeling: None,
                    modality: Modality::Text,
                    content: serde_json::to_string(&input_chat)?,
                };
                serde_json::to_string(&chat)?
            }
        };

        let output_chat: OpenAiChatOutput = serde_json::from_str(&output_chat_json)?;

        let assistant_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            modality: output_chat.modality,
            role: messages::Role::Assistant,
            user: ASSISTANT_NAME.to_string(),
            chat: output_chat_json.clone(),
        };
        self.repository.write().await.append(assistant_record)?;

        Ok(output_chat)
    }

    fn to_event(&self, output_chat: OpenAiChatOutput) -> Option<Event> {
        match output_chat.modality {
            Modality::Audio => Some(Event::AssistantSpeech {
                message: output_chat.content,
            }),
            Modality::Text => Some(Event::AssistantText {
                message: output_chat.content,
            }),
            Modality::Code => Some(Event::CodeExecutionRequest {
                code: output_chat.content,
            }),
            _ => {
                println!("unsupported modality: {:?}", output_chat.modality);
                None
            }
        }
    }

    async fn run_internal(
        &mut self,
        sender: Sender<Event>,
        mut receiver: Receiver<Event>,
    ) -> Result<(), Error> {
        loop {
            let event = receiver.recv().await?;
            let input_chat = match event {
                Event::RecognizedSpeech { user, message } => Some(OpenAiChatInput {
                    modality: Modality::Audio,
                    user,
                    content: message,
                }),
                Event::TextMessage { user, message } => Some(OpenAiChatInput {
                    modality: Modality::Text,
                    user,
                    content: message,
                }),
                _ => None,
            };
            if let Some(input_chat) = input_chat {
                let output_chat = self.receive(input_chat).await?;
                if let Some(event) = self.to_event(output_chat) {
                    sender.send(event)?;
                }
            }
        }
    }
}

#[async_trait]
impl EventComponent for OpenAiCore {
    async fn run(&mut self, sender: Sender<Event>) -> Result<(), crate::events::Error> {
        let receiver = sender.subscribe();
        self.run_internal(sender, receiver)
            .await
            .map_err(|e| events::Error::Component(format!("core: {}", e)))
    }
}
