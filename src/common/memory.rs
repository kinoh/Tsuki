use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryRecord {
    pub timestamp: u64,
    pub content: Vec<String>,
}
