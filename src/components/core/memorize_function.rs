use crate::adapter::openai::Function;
use crate::common::memory::MemoryRecord;
use crate::repository::Repository;
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;

use super::get_timestamp;

#[derive(Deserialize)]
pub struct MemorizeFunctionArguments {
    pub items: Vec<String>,
}

pub struct MemorizeFunction {
    pub repository: Arc<RwLock<Box<dyn Repository>>>,
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
            .await
            .map_err(|e| e.to_string())?;
        Ok("success".to_string())
    }
}
