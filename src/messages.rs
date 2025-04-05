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
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Role {
    System,
    Assistant,
    User,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Modality {
    Bare,
    Text,
    Audio,
    Code,
    Memory,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageRecord {
    pub timestamp: u64,
    pub modality: Modality,
    pub role: Role,
    pub user: String,
    pub chat: String,
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
            if let Ok(record) = serde_json::from_str(&line) {
                data.push(record);
            }
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
            modality: Modality::Bare,
            role: Role::System,
            user: "".to_string(),
            chat: message.to_string(),
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

    pub fn get_latest_n(&self, n: usize) -> Vec<&MessageRecord> {
        let start = self.data.len().saturating_sub(n);
        self.data
            .iter()
            .enumerate()
            .filter(|(i, r)| {
                r.role == Role::System || r.modality == Modality::Memory || *i >= start
            })
            .map(|(_, r)| r)
            .collect()
    }
}
