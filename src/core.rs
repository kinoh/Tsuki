use crate::events::{self, Event, EventComponent};
use crate::messages::{self, MessageRecord, MessageRepository};
use async_trait::async_trait;
use openai_api_rust::chat::{ChatApi, ChatBody};
use openai_api_rust::{Auth, Message, OpenAI, Role};
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

const ASSISTANT_NAME: &str = "つき";

pub enum Model {
    Echo,
    OpenAi(String),
}

fn convert_role(role: messages::Role) -> Role {
    match role {
        messages::Role::Assistant => Role::Assistant,
        messages::Role::System => Role::System,
        messages::Role::User => Role::User,
    }
}

pub struct OpenAiCore {
    repository: RwLock<MessageRepository>,
    openai: OpenAI,
    model: Model,
    max_tokens: i32,
}

impl OpenAiCore {
    pub fn new(repository: RwLock<MessageRepository>, model: Model) -> Result<Self, Error> {
        let auth = Auth::from_env().map_err(|e| Error::OpenAi(format!("auth: {}", e)))?;
        let openai = OpenAI::new(auth, "https://api.openai.com/v1/");

        Ok(Self {
            repository,
            openai,
            model,
            max_tokens: 1000,
        })
    }

    async fn receive(&mut self, user: String, message: String) -> Result<String, Error> {
        let user_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            role: messages::Role::User,
            user: user.clone(),
            message: message.clone(),
        };
        self.repository.write().await.append(user_record)?;

        let response_message = match &self.model {
            Model::Echo => format!("{}> {}", user, message),
            Model::OpenAi(model) => {
                let records = self.repository.read().await.get_all().to_vec();
                let mut messages = Vec::with_capacity(records.len());
                for r in records {
                    messages.push(Message {
                        role: convert_role(r.role),
                        content: r.message,
                    });
                }
                let chat_body = ChatBody {
                    model: model.clone(),
                    messages,
                    user: None,
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
        };

        let assistant_record = MessageRecord {
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
            role: messages::Role::Assistant,
            user: ASSISTANT_NAME.to_string(),
            message: response_message.clone(),
        };
        self.repository.write().await.append(assistant_record)?;

        Ok(response_message)
    }

    async fn run_internal(
        &mut self,
        sender: Sender<Event>,
        receiver: &mut Receiver<Event>,
    ) -> Result<(), Error> {
        loop {
            let event = receiver.recv().await?;
            match event {
                Event::RecognizedSpeech { user, message } => {
                    let response = self.receive(user, message).await?;
                    sender.send(Event::AssistantMessageIntent { message: response })?;
                }
                _ => {}
            }
        }
    }
}

#[async_trait]
impl EventComponent for OpenAiCore {
    async fn run(
        &mut self,
        sender: Sender<Event>,
        receiver: &mut Receiver<Event>,
    ) -> Result<(), crate::events::Error> {
        self.run_internal(sender, receiver)
            .await
            .map_err(|e| events::Error::Component(format!("core: {}", e)))
    }
}
