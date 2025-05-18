use crate::adapter::dify::{self, CodeExecutor};
use crate::adapter::openai;
use crate::adapter::openai::{Function, Thinker};
use crate::common::broadcast::{self, IdentifiedBroadcast};
use crate::common::chat::{
    ChatInput, ChatInputMessage, ChatOutput, ChatOutputFunctionCall, ChatOutputMessage, Modality,
};
use crate::common::events::{self, Event, EventComponent};
use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, MessageRecordChat, SYSTEM_USER_NAME};
use crate::common::repository::{self, Repository};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, SystemTimeError};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum Error {
    #[error("system time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Dify error: {0}")]
    Dify(#[from] dify::Error),
    #[error("repository error: {0}")]
    Repository(#[from] repository::Error),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
    #[error("OpenAI error: {0}")]
    OpenAi(#[from] openai::Error),
}

#[derive(PartialEq, Eq, Hash, Clone)]
pub enum DefinedMessage {
    FinishSession,
}

impl ToString for DefinedMessage {
    fn to_string(&self) -> String {
        String::from("session finished")
    }
}

pub enum Model {
    Echo,
    OpenAi(String),
}

fn to_event(output: &ChatOutput, usage: u32) -> Option<Event> {
    match output {
        ChatOutput::Message(message) => {
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
        ChatOutput::FunctionCall(call) => Some(Event::AssistantMessage {
            modality: Modality::Text,
            message: serde_json::to_string(call).unwrap_or("<serialization failed>".to_string()),
            usage,
        }),
        ChatOutput::BuiltinToolCall(call) => Some(Event::AssistantMessage {
            modality: Modality::Text,
            message: serde_json::to_string(call).unwrap_or("<serialization failed>".to_string()),
            usage,
        }),
    }
}

fn get_timestamp() -> Result<u64, SystemTimeError> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs())
}

#[derive(Deserialize)]
struct MemorizeFunctionArguments {
    items: Vec<String>,
}

struct MemorizeFunction {
    repository: Arc<RwLock<Repository>>,
}

#[async_trait]
impl Function for MemorizeFunction {
    fn name(&self) -> &'static str {
        "memorize"
    }

    fn description(&self) -> &'static str {
        "Save knowledge to memories section in instruction"
    }

    fn args_schema(&self) -> serde_json::Value {
        serde_json::json!({
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
        })
    }

    async fn call(&self, args_json: &str) -> Result<String, String> {
        let args: MemorizeFunctionArguments =
            serde_json::from_str(&args_json).map_err(|_| "invalid arguments".to_string())?;
        let timestamp = get_timestamp().unwrap_or_else(|e| {
            error!("timestamp error: {:?}", e);
            0
        });
        self.repository
            .write()
            .await
            .append_memory(MemoryRecord {
                content: args.items,
                timestamp,
            })
            .map_err(|e| e.to_string())?;
        Ok("success".to_string())
    }
}

#[derive(Deserialize)]
struct ExecuteCodeFunctionArguments {
    code: String,
}

struct ExecuteCodeFunction {
    client: CodeExecutor,
}

#[async_trait]
impl Function for ExecuteCodeFunction {
    fn name(&self) -> &'static str {
        "execute_code"
    }

    fn description(&self) -> &'static str {
        "Execute Python code. you must print() output; only stdout is returned. available packages: requests certifi beautifulsoup4 numpy scipy pandas scikit-learn matplotlib lxml pypdf"
    }

    fn args_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "code to execute"
            }
            },
            "required": ["code"],
            "additionalProperties": false
        })
    }

    async fn call(&self, args_json: &str) -> Result<String, String> {
        let args: ExecuteCodeFunctionArguments =
            serde_json::from_str(&args_json).map_err(|_| "invalid arguments".to_string())?;
        self.client
            .execute(&args.code)
            .await
            .map_err(|e| e.to_string())
    }
}

pub struct OpenAiCore {
    repository: Arc<RwLock<Repository>>,
    thinker: Thinker,
    model: Model,
    defined_messages: HashMap<DefinedMessage, String>,
    max_tokens: u32,
}

impl OpenAiCore {
    pub async fn new(
        repository: Arc<RwLock<Repository>>,
        prompt_key: &str,
        model: Model,
        openai_api_key: &str,
        dify_sandbox_host: Option<&str>,
        dify_sandbox_api_key: &str,
    ) -> Result<Self, Error> {
        let mut thinker = Thinker::new(prompt_key, openai_api_key)?;

        thinker.register_function(MemorizeFunction {
            repository: repository.clone(),
        });
        if let Some(host) = dify_sandbox_host {
            thinker.register_function(ExecuteCodeFunction {
                client: CodeExecutor::new(host, dify_sandbox_api_key)?,
            });
        }

        let mut defined_messages = HashMap::new();
        for m in vec![DefinedMessage::FinishSession] {
            let s = m.to_string();
            defined_messages.insert(m, s);
        }

        Ok(Self {
            repository,
            thinker,
            model,
            defined_messages,
            max_tokens: 1000,
        })
    }

    async fn think_openai(
        &self,
        model: &String,
        input_chats: Vec<ChatInput>,
        previous_id: Option<&String>,
    ) -> Result<(Vec<ChatOutput>, String, u32), Error> {
        let memories = self
            .repository
            .read()
            .await
            .memories()
            .iter()
            .flat_map(|r| r.content.clone())
            .collect::<Vec<String>>();

        Ok(self
            .thinker
            .think(model, memories, self.max_tokens, input_chats, previous_id)
            .await?)
    }

    fn think_echo(
        &self,
        input_chats: Vec<ChatInput>,
    ) -> Result<(Vec<ChatOutput>, String, u32), Error> {
        let chat = if let ChatInput::Message(ref message) = input_chats[0] {
            if message.modality == Modality::Audio {
                ChatOutput::Message(ChatOutputMessage {
                    activity: None,
                    feeling: None,
                    modality: Modality::Audio,
                    content: Some(message.content.clone()),
                })
            } else if message.content.starts_with("call ") {
                let parts: Vec<&str> = message.content.splitn(3, " ").collect();
                ChatOutput::FunctionCall(ChatOutputFunctionCall {
                    call_id: "call_xxx".to_string(),
                    name: parts.get(1).unwrap_or(&"-").to_string(),
                    args: parts.get(2).unwrap_or(&"-").to_string(),
                })
            } else {
                ChatOutput::Message(ChatOutputMessage {
                    activity: None,
                    feeling: None,
                    modality: Modality::Text,
                    content: Some(serde_json::to_string(&message)?),
                })
            }
        } else {
            ChatOutput::Message(ChatOutputMessage {
                activity: None,
                feeling: None,
                modality: Modality::Text,
                content: Some("no message".to_string()),
            })
        };
        Ok((vec![chat], "".to_string(), 0))
    }

    async fn think_and_save(
        &mut self,
        input_chats: Vec<ChatInput>,
    ) -> Result<(Vec<ChatOutput>, u32), Error> {
        let previous_id = {
            let mut repo = self.repository.write().await;

            let previous_id = repo.last_response_id().cloned();

            let user_record = MessageRecord {
                timestamp: get_timestamp()?,
                chat: MessageRecordChat::Input(input_chats.clone()),
                response_id: None,
                session: repo.get_or_create_session()?,
                usage: 0,
            };
            repo.append_message(user_record)?;

            previous_id
        };

        let (outputs, response_id, usage) = match &self.model {
            Model::OpenAi(model) => {
                self.think_openai(&model, input_chats, previous_id.as_ref())
                    .await
            }
            Model::Echo => self.think_echo(input_chats),
        }?;

        {
            let mut repo = self.repository.write().await;
            let assistant_record = MessageRecord {
                timestamp: get_timestamp()?,
                chat: MessageRecordChat::Output(outputs.clone()),
                response_id: Some(response_id),
                session: repo.get_or_create_session()?,
                usage,
            };
            repo.append_message(assistant_record)?;
        }

        Ok((outputs, usage))
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
                if let ChatOutput::FunctionCall(ref call) = output {
                    call_outputs.push(ChatInput::FunctionCall(self.thinker.do_call(call).await));
                }
                if let Some(event) = to_event(&output, usage) {
                    broadcast.send(event)?;
                }
            }
            if call_outputs.is_empty() {
                break;
            }
            inputs = call_outputs;
        }

        Ok(())
    }

    async fn receive_defined(
        &mut self,
        broadcast: &IdentifiedBroadcast<Event>,
        message: DefinedMessage,
    ) -> Result<(), Error> {
        match message {
            DefinedMessage::FinishSession => {
                if self.repository.read().await.has_session() {
                    self.receive(
                        &broadcast,
                        ChatInputMessage {
                            modality: Modality::Text,
                            user: SYSTEM_USER_NAME.to_string(),
                            content: self.defined_messages.get(&message).unwrap().to_string(),
                        },
                    )
                    .await?;
                    self.repository.write().await.clear_session()?;
                }
                Ok(())
            }
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
                Event::TextMessage { user, message } => {
                    if user == SYSTEM_USER_NAME {
                        if let Some(m) = self
                            .defined_messages
                            .iter()
                            .find(|(_, s)| **s == message)
                            .map(|(m, _)| m)
                            .cloned()
                        {
                            self.receive_defined(&broadcast, m).await?;
                            continue;
                        }
                    }
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
