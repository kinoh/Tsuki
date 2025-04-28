use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::chat::{ChatInput, ChatOutput};

#[derive(Error, Debug)]
pub enum Error {
    #[error("stserde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
}

pub const ASSISTANT_NAME: &str = "つき";
pub const SYSTEM_USER_NAME: &str = "system";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageRecordChat {
    Input(Vec<ChatInput>),
    Output(Vec<ChatOutput>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageRecord {
    pub timestamp: u64,
    pub chat: MessageRecordChat,
    pub response_id: Option<String>,
    pub usage: u32,
}

impl MessageRecord {
    pub fn user(&self) -> String {
        match self.chat {
            MessageRecordChat::Input(ref chats) => match chats.get(0) {
                Some(chat) => match chat {
                    ChatInput::Message(message) => message.user.clone(),
                    _ => SYSTEM_USER_NAME.to_string(),
                },
                None => "".to_string(),
            },
            MessageRecordChat::Output(_) => ASSISTANT_NAME.to_string(),
        }
    }

    pub fn json_chat(&self) -> Result<Value, serde_json::Error> {
        Ok(match self.chat {
            MessageRecordChat::Input(ref chat) => serde_json::to_value(chat)?,
            MessageRecordChat::Output(ref chat) => serde_json::to_value(chat)?,
        })
    }
}
