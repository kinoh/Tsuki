use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Modality {
    None,
    Text,
    Audio,
    Code,
    Tick,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatInputMessage {
    pub modality: Modality,
    pub user: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatInputFunctionCall {
    pub call_id: String,
    pub output: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ChatInput {
    Message(ChatInputMessage),
    FunctionCall(ChatInputFunctionCall),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatOutputMessage {
    pub feeling: Option<u8>,
    pub activity: Option<u8>,
    pub modality: Modality,
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatOutputFunctionCall {
    pub call_id: String,
    pub name: String,
    pub args: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ChatOutput {
    Message(ChatOutputMessage),
    FunctionCall(ChatOutputFunctionCall),
    BuiltinToolCall(Value),
}
