use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use thiserror::Error;

use super::chat::{ChatInput, ChatOutput, Modality};

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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageRecordChat {
    Input(ChatInput),
    Output(ChatOutput),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageRecord {
    pub timestamp: u64,
    pub chat: MessageRecordChat,
    pub response_id: Option<String>,
    pub usage: u32,
}

impl MessageRecord {
    pub fn modality(&self) -> Modality {
        match self.chat {
            MessageRecordChat::Input(ref chat) => chat.modality,
            MessageRecordChat::Output(ref chat) => chat.modality,
        }
    }

    pub fn user(&self) -> String {
        match self.chat {
            MessageRecordChat::Input(ref chat) => chat.user.to_string(),
            MessageRecordChat::Output(_) => ASSISTANT_NAME.to_string(),
        }
    }

    pub fn json_chat(&self) -> Result<Value, serde_json::Error> {
        Ok(match self.chat {
            MessageRecordChat::Input(ref chat) => serde_json::to_value(chat)?,
            MessageRecordChat::Output(ref chat) => serde_json::to_value(chat)?,
        })
    }
}

pub struct MessageRepository {
    data: Vec<MessageRecord>,
    writer: std::fs::File,
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
            } else {
                return Err(Error::Migration);
            };
            data.push(record);
        }

        Ok(Self { data, writer: file })
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
            let is_important = record.modality() == Modality::Memory;
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
