use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("std::io error: {0}")]
    StdIo(#[from] std::io::Error),
    #[error("stserde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("migration error")]
    Migration,
}

pub const ASSISTANT_NAME: &str = "つき";
pub const SYSTEM_USER_NAME: &str = "system";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Role {
    System,
    Assistant,
    User,
}

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
pub struct OldMessageRecord {
    pub timestamp: u64,
    pub modality: Modality,
    pub role: Role,
    pub user: String,
    pub chat: String,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageRecordChat {
    Input(ChatInput),
    Output(ChatOutput),
    Bare(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageRecord {
    pub timestamp: u64,
    pub role: Role,
    pub chat: MessageRecordChat,
}

impl MessageRecord {
    pub fn modality(&self) -> Modality {
        match self.chat {
            MessageRecordChat::Bare(_) => Modality::Bare,
            MessageRecordChat::Input(ref chat) => chat.modality,
            MessageRecordChat::Output(ref chat) => chat.modality,
        }
    }

    pub fn user(&self) -> String {
        match self.chat {
            MessageRecordChat::Bare(_) => "".to_string(),
            MessageRecordChat::Input(ref chat) => chat.user.to_string(),
            MessageRecordChat::Output(_) => ASSISTANT_NAME.to_string(),
        }
    }

    pub fn json_chat(&self) -> Result<String, serde_json::Error> {
        Ok(match self.chat {
            MessageRecordChat::Bare(ref chat) => chat.clone(),
            MessageRecordChat::Input(ref chat) => serde_json::to_string(chat)?,
            MessageRecordChat::Output(ref chat) => serde_json::to_string(chat)?,
        })
    }
}

pub struct MessageRepository {
    data: Vec<MessageRecord>,
    writer: std::fs::File,
    path: String,
}

impl MessageRepository {
    pub fn new(path: String) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;

        let reader = BufReader::new(File::open(&path)?);
        let mut data = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let record = if let Ok(record) = serde_json::from_str(&line) {
                record
            } else if let Ok(old) = serde_json::from_str::<OldMessageRecord>(&line) {
                MessageRecord {
                    timestamp: old.timestamp,
                    role: old.role,
                    chat: if old.modality == Modality::Bare {
                        MessageRecordChat::Bare(old.chat)
                    } else if old.role == Role::Assistant {
                        MessageRecordChat::Output(serde_json::from_str(&old.chat)?)
                    } else {
                        MessageRecordChat::Input(serde_json::from_str(&old.chat)?)
                    },
                }
            } else {
                return Err(Error::Migration);
            };
            data.push(record);
        }

        Ok(Self {
            data,
            writer: file,
            path,
        })
    }

    pub fn load_initial_prompt(&mut self, message: &str) -> Result<(), Error> {
        let record = MessageRecord {
            timestamp: 0,
            role: Role::System,
            chat: MessageRecordChat::Bare(message.to_string()),
        };
        if self.data.len() == 0 {
            self.data.push(record);
        } else {
            self.data[0] = record;
        }

        self.writer = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)?;

        for record in &self.data {
            let json = serde_json::to_string(&record)?;
            writeln!(self.writer, "{}", json)?;
        }
        self.writer.flush()?;

        Ok(())
    }

    pub fn append(&mut self, record: MessageRecord) -> Result<(), Error> {
        let json = serde_json::to_string(&record)?;
        writeln!(self.writer, "{}", json)?;
        self.writer.flush()?;
        self.data.push(record);
        Ok(())
    }

    pub fn get_all(&self) -> &[MessageRecord] {
        &self.data
    }

    pub fn get_latest_n(&self, n: usize, before: Option<u64>) -> Vec<&MessageRecord> {
        let mut response = Vec::with_capacity(n);
        let mut is_last_none = false;
        let mut normal_count = 0;
        for i in 1..=self.data.len() {
            let record = &self.data[self.data.len() - i];
            let is_none = record.modality() == Modality::None;
            let is_important = record.role == Role::System || record.modality() == Modality::Memory;
            if !is_none
                && !(record.modality() == Modality::Tick && is_last_none)
                && (is_important || normal_count < n)
                && (before.is_none_or(|t| record.timestamp < t))
            {
                response.push(record);
                if !is_important {
                    normal_count += 1;
                }
            }
            is_last_none = is_none;
        }
        response.reverse();
        response
    }
}
