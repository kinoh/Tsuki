use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Modality {
    None,
    Bare,
    Text,
    Audio,
    Code,
    Memory,
    Tick,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatInput {
    pub modality: Modality,
    pub user: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatOutput {
    pub feeling: Option<u8>,
    pub activity: Option<u8>,
    pub modality: Modality,
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct TokenUsage {
    pub prompt: u32,
    pub completion: u32,
}
