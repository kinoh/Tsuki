use anyhow::{Context, Result};
use async_trait::async_trait;
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ErrorKind};
use tracing::info;
use uuid::Uuid;

use super::Repository;
use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, SessionId};
use crate::common::schedule::ScheduleRecord;

#[derive(Serialize, Deserialize, Default, Clone)]
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
    data: tokio::sync::RwLock<RepositoryData>,
}

impl FileRepository {
    pub async fn new(path: &str, pretty: bool) -> Result<Self> {
        let data = match File::open(path).await {
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    info!(path = path, "data file not found");
                    RepositoryData::default()
                } else {
                    return Err(anyhow::anyhow!(e));
                }
            }
            Ok(mut file) => {
                let mut buf = String::new();
                file.read_to_string(&mut buf).await?;
                if buf.is_empty() {
                    RepositoryData::default()
                } else {
                    serde_json::from_str(&buf)?
                }
            }
        };

        Ok(Self {
            path: path.to_string(),
            pretty,
            data: tokio::sync::RwLock::new(data),
        })
    }

    async fn save(&self, data: &RepositoryData) -> Result<()> {
        let json = if self.pretty {
            serde_json::to_string_pretty(data)
        } else {
            serde_json::to_string(data)
        }?;
        let mut file = File::create(self.path.clone())
            .await
            .context("failed to create file")?;
        file.write_all(json.as_bytes())
            .await
            .context("failed to write json")?;
        Ok(())
    }
}

#[async_trait]
impl Repository for FileRepository {
    async fn get_or_create_session(&self) -> Result<SessionId> {
        let mut data = self.data.write().await;
        if let Some(ref session) = data.current_session {
            Ok(session.clone())
        } else {
            let session = Uuid::new_v4().simple().to_string();
            data.current_session = Some(session.clone());
            self.save(&data).await?;
            Ok(session)
        }
    }

    async fn has_session(&self) -> bool {
        self.data.read().await.current_session.is_some()
    }

    async fn clear_session(&self) -> Result<()> {
        let mut data = self.data.write().await;
        data.current_session = None;
        self.save(&data).await
    }

    async fn append_message(&self, record: MessageRecord) -> Result<()> {
        let mut data = self.data.write().await;
        data.messages.push(record);
        self.save(&data).await
    }

    async fn messages(
        &self,
        latest_n: Option<usize>,
        before: Option<u64>,
    ) -> Result<Vec<MessageRecord>> {
        let data = self.data.read().await;
        let total = data.messages.len();
        let mut response = Vec::with_capacity(latest_n.unwrap_or(total));
        for i in 1..=total {
            let record = &data.messages[total - i];
            if latest_n.is_none() || response.len() < latest_n.unwrap() {
                if before.is_none() || record.timestamp < before.unwrap() {
                    response.push(record.clone());
                }
            }
        }
        response.reverse();
        Ok(response)
    }

    async fn last_response_id(&self) -> Result<Option<String>> {
        let data = self.data.read().await;
        let response = if let Some(ref _session) = data.current_session {
            data.messages
                .iter()
                .rev()
                .find_map(|r| r.response_id.clone())
        } else {
            None
        };
        Ok(response)
    }

    async fn append_memory(&self, record: MemoryRecord) -> Result<()> {
        let mut data = self.data.write().await;
        data.memories.push(record);
        self.save(&data).await
    }

    async fn memories(&self, _query: &str) -> Result<Vec<MemoryRecord>> {
        let data = self.data.read().await;
        Ok(data.memories.clone())
    }

    async fn append_schedule(
        &self,
        expression: String,
        message: String,
    ) -> Result<()> {
        let schedule = Schedule::from_str(&expression).context("failed to parse schedule")?;
        let mut data = self.data.write().await;
        data.schedules.push(ScheduleRecord { schedule, message });
        self.save(&data).await
    }

    async fn remove_schedule(
        &self,
        expression: String,
        message: String,
    ) -> Result<usize> {
        let schedule = Schedule::from_str(&expression).context("failed to parse schedule")?;
        let mut data = self.data.write().await;
        let initial_len = data.schedules.len();
        data.schedules
            .retain(|s| !(s.schedule == schedule && s.message == message));
        let removed_count = initial_len - data.schedules.len();
        if removed_count > 0 {
            self.save(&data).await?;
        }
        Ok(removed_count)
    }

    async fn schedules(&self) -> Result<Vec<ScheduleRecord>> {
        let data = self.data.read().await;
        Ok(data.schedules.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn remove_schedule() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let repo = FileRepository::new(path, false).await.unwrap();
        let expr = "0 5 * * * *".to_string();
        let msg = "test message".to_string();

        repo.append_schedule(expr.clone(), msg.clone())
            .await
            .unwrap();
        let schedules = repo.schedules().await.unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].message, msg);
        assert_eq!(schedules[0].schedule.to_string(), expr);

        let removed = repo
            .remove_schedule(expr.clone(), msg.clone())
            .await
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(repo.schedules().await.unwrap().len(), 0);
    }
}
