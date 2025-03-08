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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Role {
    System,
    Assistant,
    User,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageRecord {
    pub timestamp: u64,
    pub role: Role,
    pub user: String,
    pub message: String,
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
            if let Ok(record) = serde_json::from_str(&line) {
                data.push(record);
            }
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
}
