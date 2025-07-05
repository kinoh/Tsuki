mod file;

use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, SessionId};
use crate::common::schedule::ScheduleRecord;

pub use file::FileRepository;

#[derive(Debug, thiserror::Error)]
#[error("{component} error: {source}")]
pub struct Error {
    component: String,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

use async_trait::async_trait;

#[async_trait]
pub trait Repository: Send + Sync {
    async fn get_or_create_session(&self) -> Result<SessionId, Error>;
    async fn has_session(&self) -> bool;
    async fn clear_session(&self) -> Result<(), Error>;
    async fn append_message(&self, record: MessageRecord) -> Result<(), Error>;
    async fn messages(
        &self,
        latest_n: Option<usize>,
        before: Option<u64>,
    ) -> Result<Vec<MessageRecord>, Error>;
    async fn last_response_id(&self) -> Result<Option<String>, Error>;
    async fn append_memory(&self, record: MemoryRecord) -> Result<(), Error>;
    async fn memories(&self, query: &str) -> Result<Vec<MemoryRecord>, Error>;
    async fn append_schedule(&self, expression: String, message: String) -> Result<(), Error>;
    async fn remove_schedule(&self, expression: String, message: String) -> Result<usize, Error>;
    async fn schedules(&self) -> Result<Vec<ScheduleRecord>, Error>;
}
