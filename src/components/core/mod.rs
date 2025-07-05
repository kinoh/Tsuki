mod execute_code_function;
mod manage_schedule_function;
mod memorize_function;

use anyhow::Result;
use async_trait::async_trait;
use execute_code_function::ExecuteCodeFunction;
use memorize_function::MemorizeFunction;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::adapter::dify::CodeExecutor;
use crate::adapter::openai::Thinker;
use crate::common::broadcast::IdentifiedBroadcast;
use crate::common::chat::{
    ChatInput, ChatInputMessage, ChatOutput, ChatOutputFunctionCall, ChatOutputMessage, Modality,
};
use crate::common::events::{Event, EventComponent};
use crate::common::message::{MessageRecord, MessageRecordChat, SYSTEM_USER_NAME};
use crate::repository::Repository;

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

fn get_timestamp() -> Result<u64, std::time::SystemTimeError> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
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

pub struct OpenAiCore {
    repository: Arc<RwLock<dyn Repository>>,
    thinker: Thinker,
    model: Model,
    defined_messages: HashMap<DefinedMessage, String>,
    max_tokens: u32,
}

impl OpenAiCore {
    pub async fn new(
        repository: Arc<RwLock<dyn Repository>>,
        prompt_key: &str,
        model: Model,
        openai_api_key: &str,
        dify_sandbox_host: Option<&str>,
        dify_sandbox_api_key: &str,
    ) -> Result<Self> {
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
    ) -> Result<(Vec<ChatOutput>, String, u32)> {
        let memories = self
            .repository
            .read()
            .await
            .memories("") // TODO: pass query
            .await?
            .iter()
            .flat_map(|r| r.content.clone())
            .collect::<Vec<String>>();

        Ok(self
            .thinker
            .think(model, memories, self.max_tokens, input_chats, previous_id)
            .await?)
    }

    fn think_echo(&self, input_chats: Vec<ChatInput>) -> Result<(Vec<ChatOutput>, String, u32)> {
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
                    content: Some(serde_json::to_string(&message).unwrap()),
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
    ) -> Result<(Vec<ChatOutput>, u32)> {
        let previous_id = {
            let repo = self.repository.write().await;

            let previous_id = repo.last_response_id().await?;

            let user_record = MessageRecord {
                timestamp: get_timestamp()?,
                chat: MessageRecordChat::Input(input_chats.clone()),
                response_id: None,
                session: repo.get_or_create_session().await?,
                usage: 0,
            };
            repo.append_message(user_record).await?;

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
            let repo = self.repository.write().await;
            let assistant_record = MessageRecord {
                timestamp: get_timestamp()?,
                chat: MessageRecordChat::Output(outputs.clone()),
                response_id: Some(response_id),
                session: repo.get_or_create_session().await?,
                usage,
            };
            repo.append_message(assistant_record).await?;
        }

        Ok((outputs, usage))
    }

    async fn receive(
        &mut self,
        broadcast: &IdentifiedBroadcast<Event>,
        message: ChatInputMessage,
    ) -> Result<()> {
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
    ) -> Result<()> {
        match message {
            DefinedMessage::FinishSession => {
                if self.repository.read().await.has_session().await {
                    self.receive(
                        &broadcast,
                        ChatInputMessage {
                            modality: Modality::Text,
                            user: SYSTEM_USER_NAME.to_string(),
                            content: self.defined_messages.get(&message).unwrap().to_string(),
                        },
                    )
                    .await?;
                    self.repository.write().await.clear_session().await?;
                }
                Ok(())
            }
        }
    }

    async fn run_internal(&mut self, mut broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        info!("start core");

        // self.thinker.register_function(ManageScheduleFunction {
        //     repository: self.repository.clone(),
        //     broadcast: broadcast.clone(),
        // });

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
    async fn run(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| anyhow::anyhow!("core: {}", e))
    }
}
