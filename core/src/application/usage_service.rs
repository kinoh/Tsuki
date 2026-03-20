use async_trait::async_trait;
use std::sync::Arc;

use crate::clock::now_iso8601;
use crate::db::{Db, UsageStatRecord};
use crate::llm::{LlmUsage, LlmUsageContext, LlmUsageRecorder};

pub(crate) struct DbLlmUsageRecorder {
    db: Arc<Db>,
}

impl DbLlmUsageRecorder {
    pub(crate) fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl LlmUsageRecorder for DbLlmUsageRecorder {
    async fn record_usage(
        &self,
        response_id: &str,
        usage: &LlmUsage,
        context: &LlmUsageContext,
    ) -> Result<(), String> {
        if response_id.trim().is_empty() {
            return Ok(());
        }

        let record = UsageStatRecord {
            id: response_id.to_string(),
            user_id: context.user_id.clone(),
            agent_name: context.agent_name.clone(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            created_at: now_iso8601(),
        };

        self.db
            .insert_usage_stat(record)
            .await
            .map_err(|err| err.to_string())
    }
}
