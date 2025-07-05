use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

use super::{Error, Repository};
use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, SessionId};
use crate::common::schedule::ScheduleRecord;

pub trait WrapErrorExt<T> {
    fn wrap(self) -> Result<T, Error>;
}

impl<T, E> WrapErrorExt<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn wrap(self) -> Result<T, Error> {
        self.map_err(|e| Error {
            component: String::from("file"),
            source: Box::new(e),
        })
    }
}

#[derive(Serialize, Deserialize, Default)]
struct RepositoryData {
    #[serde(default)]
    current_session: Option<SessionId>,
    #[serde(default)]
    messages: Vec<MessageRecord>,
    #[serde(default)]
    memories: Vec<MemoryRecord>,
    #[serde(default)]
    schedules: Vec<ScheduleRecord>,
}

pub struct FileRepository {
    path: String,
    pretty: bool,
    data: RepositoryData,
}

impl FileRepository {
    pub fn new(path: &str, pretty: bool) -> Result<Self, Error> {
        let data = match File::open(path) {
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    info!(path = path, "data file not found");
                    RepositoryData::default()
                } else {
                    return Err(e).wrap();
                }
            }
            Ok(mut file) => {
                let mut buf = String::new();
                file.read_to_string(&mut buf).wrap()?;
                if buf.is_empty() {
                    RepositoryData::default()
                } else {
                    serde_json::from_str(&buf).wrap()?
                }
            }
        };

        Ok(Self {
            path: path.to_string(),
            pretty,
            data,
        })
    }

    fn save(&mut self) -> Result<(), Error> {
        let json = if self.pretty {
            serde_json::to_string_pretty(&self.data)
        } else {
            serde_json::to_string(&self.data)
        }
        .wrap()?;
        let mut file = File::create(self.path.clone()).wrap()?;
        file.write_all(json.as_bytes()).wrap()?;
        Ok(())
    }
}

impl Repository for FileRepository {
    fn get_or_create_session(&mut self) -> Result<SessionId, Error> {
        if let Some(ref session) = self.data.current_session {
            Ok(session.clone())
        } else {
            let session = Uuid::new_v4().simple().to_string();
            self.data.current_session = Some(session.clone());
            self.save()?;
            Ok(session)
        }
    }

    fn has_session(&self) -> bool {
        self.data.current_session.is_some()
    }

    fn clear_session(&mut self) -> Result<(), Error> {
        self.data.current_session = None;
        self.save()
    }

    fn append_message(&mut self, record: MessageRecord) -> Result<(), Error> {
        self.data.messages.push(record);
        self.save()
    }

    fn messages(&self, latest_n: Option<usize>, before: Option<u64>) -> Vec<&MessageRecord> {
        let total = self.data.messages.len();
        let mut response = Vec::with_capacity(latest_n.unwrap_or(total));
        for i in 1..=total {
            let record = &self.data.messages[total - i];
            if latest_n.is_none_or(|n| response.len() < n)
                && before.is_none_or(|t| record.timestamp < t)
            {
                response.push(record);
            }
        }
        response.reverse();
        response
    }

    fn last_response_id(&self) -> Option<&String> {
        if let Some(ref session) = self.data.current_session {
            self.data
                .messages
                .iter()
                .rev()
                .filter(|r| r.session == *session)
                .find_map(|r| r.response_id.as_ref())
        } else {
            None
        }
    }

    fn append_memory(&mut self, record: MemoryRecord) -> Result<(), Error> {
        self.data.memories.push(record);
        self.save()
    }

    fn memories(&self) -> Vec<&MemoryRecord> {
        self.data.memories.iter().map(|m| m).collect()
    }

    fn append_schedule(&mut self, expression: String, message: String) -> Result<(), Error> {
        let schedule = Schedule::from_str(&expression).wrap()?;
        self.data
            .schedules
            .push(ScheduleRecord { schedule, message });
        self.save()
    }

    fn remove_schedule(&mut self, expression: String, message: String) -> Result<usize, Error> {
        let schedule = Schedule::from_str(&expression).wrap()?;
        let indices = self
            .data
            .schedules
            .iter()
            .enumerate()
            .filter(|(_, s)| s.schedule == schedule && s.message == message)
            .map(|(i, _)| i)
            .collect::<Vec<usize>>();
        for i in &indices {
            self.data.schedules.remove(*i);
        }
        Ok(indices.len())
    }

    fn schedules(&self) -> Vec<&ScheduleRecord> {
        self.data.schedules.iter().map(|s| s).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn remove_schedule() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let mut repo = FileRepository::new(path, false).unwrap();
        let expr = "0 5 * * * *".to_string();
        let msg = "test message".to_string();

        repo.append_schedule(expr.clone(), msg.clone()).unwrap();
        assert_eq!(repo.schedules().len(), 1);
        assert_eq!(repo.schedules()[0].message, msg);
        assert_eq!(repo.schedules()[0].schedule.to_string(), expr);

        let removed = repo.remove_schedule(expr.clone(), msg.clone()).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(repo.schedules().len(), 0);
    }
}
