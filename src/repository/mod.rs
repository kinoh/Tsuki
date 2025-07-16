mod file;
mod qdrant;

use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
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

pub struct RepositoryFactory {
    openai_api_key: Option<String>,
}

impl RepositoryFactory {
    pub fn new() -> Self {
        Self {
            openai_api_key: None,
        }
    }
    
    pub fn with_openai_key(mut self, api_key: String) -> Self {
        self.openai_api_key = Some(api_key);
        self
    }
    
    pub async fn create(&self, database_type: &str, database_url: &str) -> Result<Arc<RwLock<Box<dyn Repository>>>> {
        match database_type {
            "file" => Ok(Arc::new(RwLock::new(Box::new(
                file::FileRepository::new(database_url, cfg!(debug_assertions)).await?,
            )))),
            "qdrant" => {
                let api_key = self.openai_api_key.as_ref()
                    .ok_or_else(|| anyhow::anyhow!("OpenAI API key required for Qdrant repository"))?;
                let embedding_service = Arc::new(
                    crate::adapter::embedding::EmbeddingService::new(api_key).await?
                );
                Ok(Arc::new(RwLock::new(Box::new(
                    qdrant::QdrantRepository::new(database_url, embedding_service).await?,
                ))))
            },
            _ => bail!("Unrecognized repository type: {}", database_type),
        }
    }
}

