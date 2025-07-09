use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{select, sync::RwLock, time};
use tracing::info;

use crate::common::broadcast::IdentifiedBroadcast;
use crate::common::events::{Event, EventComponent};
use crate::common::message::SYSTEM_USER_NAME;
use crate::common::schedule::ScheduleRecord;
use crate::repository::Repository;

fn now() -> DateTime<Utc> {
    #[cfg(test)]
    let t = mock_chrono::Utc::now();
    #[cfg(not(test))]
    let t = Utc::now();
    t
}

pub struct Scheduler {
    repository: Arc<RwLock<Box<dyn Repository>>>,
    last_sent: HashMap<ScheduleRecord, DateTime<Utc>>,
    resolution: Duration,
}

impl Scheduler {
    pub async fn new(
        repository: Arc<RwLock<Box<dyn Repository>>>,
        resolution: Duration,
    ) -> Result<Self> {
        let scheduler = Self {
            repository,
            last_sent: HashMap::new(),
            resolution,
        };
        Ok(scheduler)
    }

    pub async fn register(&mut self, expression: String, message: String) -> Result<()> {
        self.repository
            .write()
            .await
            .append_schedule(expression, message)
            .await?;
        Ok(())
    }

    async fn next(&mut self, now: DateTime<Utc>) -> Option<(ScheduleRecord, DateTime<Utc>)> {
        self.repository
            .read()
            .await
            .schedules()
            .await
            .ok()?
            .iter()
            .filter_map(|r| r.schedule.after(&now).next().map(|t| (r.clone(), t)))
            .filter(|(r, next)| self.last_sent.get(r).is_none_or(|last| *last < *next))
            .min_by_key(|(_, t)| *t)
            .map(|(r, t)| (r.clone(), t))
    }

    async fn event_ready(&mut self) -> Result<Option<(ScheduleRecord, DateTime<Utc>)>> {
        let resolution = TimeDelta::from_std(self.resolution)?;

        let now = now();

        if let Some((record, time)) = self.next(now - resolution).await {
            if time < now + resolution {
                return Ok(Some((record, time)));
            }
        }
        Ok(None)
    }

    async fn run_internal(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        info!("start scheduler");

        let mut interval = time::interval(self.resolution);

        loop {
            select! {
                _ = interval.tick() => {
                    if let Some((record, time)) = self.event_ready().await? {
                        info!("schedule triggered: {}", time);
                        broadcast.send(Event::TextMessage {
                            user: String::from(SYSTEM_USER_NAME),
                            message: record.message.clone(),
                        })?;

                        let now = now();
                        if record.schedule.after(&now).next().is_none() {
                            self.repository.write().await.remove_schedule(record.schedule.to_string(), record.message).await?;
                        } else {
                            self.last_sent.insert(record, time);
                        }
                    }
                }
            }
        }
    }
}

#[async_trait]
impl EventComponent for Scheduler {
    async fn run(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        self.run_internal(broadcast.participate())
            .await
            .context("scheduler")
    }
}

#[cfg(test)]
mod mock_chrono {
    use chrono::{DateTime, NaiveDate};
    use std::cell::Cell;

    thread_local! {
        static TIMESTAMP: Cell<i64> = const { Cell::new(0) };
    }

    pub struct Utc;

    impl Utc {
        pub fn now() -> DateTime<chrono::Utc> {
            TIMESTAMP
                .with(|timestamp| DateTime::<chrono::Utc>::from_timestamp(timestamp.get(), 0))
                .expect("invalid timestamp")
        }
    }

    pub fn set_timestamp(h: u32, m: u32, s: u32) {
        let timestamp = NaiveDate::from_ymd_opt(2025, 1, 1)
            .unwrap()
            .and_hms_opt(h, m, s)
            .unwrap()
            .and_utc()
            .timestamp();
        TIMESTAMP.with(|ts| ts.set(timestamp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn daily() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let repo = repository::generate("file", path).await.unwrap();
        let mut scheduler = Scheduler::new(repo, Duration::from_secs(60)).await.unwrap();
        scheduler
            .register(String::from("0 30 19 * * *"), String::from("foo"))
            .await
            .unwrap();
        mock_chrono::set_timestamp(19, 29, 0);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), false);
        mock_chrono::set_timestamp(19, 29, 1);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), true);
        mock_chrono::set_timestamp(19, 30, 0);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), true);
        mock_chrono::set_timestamp(19, 30, 59);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), true);
        mock_chrono::set_timestamp(19, 31, 0);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), false);
    }

    #[tokio::test]
    async fn no_duplicate() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        let repo = repository::generate("file", path).await.unwrap();
        let mut scheduler = Scheduler::new(repo, Duration::from_secs(60)).await.unwrap();
        scheduler
            .register(String::from("0 0 20,21 * * *"), String::from("foo"))
            .await
            .unwrap();
        mock_chrono::set_timestamp(20, 0, 0);
        let ready = scheduler.event_ready().await.unwrap();
        assert_eq!(ready.is_some(), true);
        let ready = ready.unwrap();
        scheduler.last_sent.insert(ready.0, ready.1);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), false);
        mock_chrono::set_timestamp(20, 59, 0);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), false);
        mock_chrono::set_timestamp(21, 00, 0);
        assert_eq!(scheduler.event_ready().await.unwrap().is_some(), true);
    }
}
