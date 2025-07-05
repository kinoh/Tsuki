pub mod file;

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

pub trait Repository: Send + Sync {
    fn get_or_create_session(&mut self) -> Result<SessionId, Error>;
    fn has_session(&self) -> bool;
    fn clear_session(&mut self) -> Result<(), Error>;
    fn append_message(&mut self, record: MessageRecord) -> Result<(), Error>;
    fn messages(&self, latest_n: Option<usize>, before: Option<u64>) -> Vec<&MessageRecord>;
    fn last_response_id(&self) -> Option<&String>;
    fn append_memory(&mut self, record: MemoryRecord) -> Result<(), Error>;
    fn memories(&self) -> Vec<&MemoryRecord>;
    fn append_schedule(&mut self, expression: String, message: String) -> Result<(), Error>;
    fn remove_schedule(&mut self, expression: String, message: String) -> Result<usize, Error>;
    fn schedules(&self) -> Vec<&ScheduleRecord>;
}
