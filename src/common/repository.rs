use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use thiserror::Error;
use tracing::info;

use super::memory::MemoryRecord;
use super::message::MessageRecord;

#[derive(Error, Debug)]
pub enum Error {
    #[error("std::io error: {0}")]
    StdIo(#[from] std::io::Error),
    #[error("stserde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
}

#[derive(Serialize, Deserialize, Default)]
struct RepositoryData {
    messages: Vec<MessageRecord>,
    memories: Vec<MemoryRecord>,
}

pub struct Repository {
    path: String,
    pretty: bool,
    data: RepositoryData,
}

impl Repository {
    pub fn new(path: String, pretty: bool) -> Result<Self, Error> {
        let data = match File::open(&path) {
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    info!(path = path, "data file not found");
                    RepositoryData::default()
                } else {
                    return Err(e.into());
                }
            }
            Ok(mut file) => {
                let mut buf = String::new();
                file.read_to_string(&mut buf)?;
                if buf.is_empty() {
                    RepositoryData::default()
                } else {
                    serde_json::from_str(&buf)?
                }
            }
        };

        Ok(Self { path, pretty, data })
    }

    fn save(&mut self) -> Result<(), Error> {
        let json = if self.pretty {
            serde_json::to_string_pretty(&self.data)
        } else {
            serde_json::to_string(&self.data)
        }?;
        let mut file = File::create(self.path.clone())?;
        file.write_all(json.as_bytes())?;
        Ok(())
    }

    pub fn append_message(&mut self, record: MessageRecord) -> Result<(), Error> {
        self.data.messages.push(record);
        self.save()
    }

    pub fn messages(&self, latest_n: Option<usize>, before: Option<u64>) -> Vec<&MessageRecord> {
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

    pub fn last_message_id(&self) -> Option<&String> {
        self.data
            .messages
            .iter()
            .rev()
            .find_map(|r| r.response_id.as_ref())
    }

    pub fn append_memory(&mut self, record: MemoryRecord) -> Result<(), Error> {
        self.data.memories.push(record);
        self.save()
    }

    pub fn memories(&self) -> Vec<&MemoryRecord> {
        self.data.memories.iter().map(|m| m).collect()
    }
}
