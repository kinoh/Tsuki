mod file;
mod weaviate;

use anyhow::{bail, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, SessionId};
use crate::common::schedule::ScheduleRecord;

#[async_trait]
pub trait Repository: Send + Sync {
    async fn get_or_create_session(&self) -> Result<SessionId>;
    async fn has_session(&self) -> bool;
    async fn clear_session(&self) -> Result<()>;
    async fn append_message(&self, record: MessageRecord) -> Result<()>;
    async fn messages(
        &self,
        latest_n: Option<usize>,
        before: Option<u64>,
    ) -> Result<Vec<MessageRecord>>;
    async fn last_response_id(&self) -> Result<Option<String>>;
    async fn append_memory(&self, record: MemoryRecord) -> Result<()>;
    async fn memories(&self, query: &str) -> Result<Vec<MemoryRecord>>;
    async fn append_schedule(&self, expression: String, message: String) -> Result<()>;
    async fn remove_schedule(&self, expression: String, message: String) -> Result<usize>;
    async fn schedules(&self) -> Result<Vec<ScheduleRecord>>;
}

pub async fn generate(name: &str, url: &str) -> Result<Arc<RwLock<Box<dyn Repository>>>> {
    match name {
        "file" => Ok(Arc::new(RwLock::new(Box::new(
            file::FileRepository::new(url, cfg!(debug_assertions)).await?,
        )))),
        "weaviate" => Ok(Arc::new(RwLock::new(Box::new(
            weaviate::WeaviateRepository::new(url).await?,
        )))),
        _ => bail!("Unrecognized repository type"),
    }
}
