use std::{str::FromStr, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use cron::Schedule;
use thiserror::Error;
use tokio::{sync::RwLock, time};
use tracing::info;

use crate::common::{
    broadcast::{self, IdentifiedBroadcast},
    events::{self, Event, EventComponent},
    message::SYSTEM_USER_NAME,
    repository::Repository,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
    #[error("cron error: {0}")]
    Cron(#[from] cron::error::Error),
    #[error("chrono out of range: {0}")]
    ChronoOutOfRange(#[from] chrono::OutOfRangeError),
    #[error("Repository error: {0}")]
    Repository(#[from] crate::common::repository::Error),
}

struct EventSchedule {
    schedule: Schedule,
    message: String,
    last_sent: DateTime<Utc>,
}

pub struct Scheduler {
    repository: Arc<RwLock<Repository>>,
    schedules: Vec<EventSchedule>,
    resolution: Duration,
}

impl Scheduler {
    pub async fn new(
        repository: Arc<RwLock<Repository>>,
        resolution: Duration,
    ) -> Result<Self, Error> {
        let mut scheduler = Self {
            repository,
            schedules: Vec::new(),
            resolution,
        };
        scheduler.load().await?;
        Ok(scheduler)
    }

    async fn load(&mut self) -> Result<(), Error> {
        let data: Result<Vec<EventSchedule>, Error> = self
            .repository
            .read()
            .await
            .schedules()
            .iter()
            .map(|r| {
                Ok(EventSchedule {
                    schedule: Schedule::from_str(&r.expression)?,
                    message: r.message.clone(),
                    last_sent: DateTime::<Utc>::from_timestamp(0, 0).unwrap(),
                })
            })
            .collect();
        self.schedules = data?;
        Ok(())
    }

    pub async fn register(&mut self, expression: String, message: String) -> Result<(), Error> {
        self.repository
            .write()
            .await
            .append_schedule(expression, message)?;
        self.load().await?;
        Ok(())
    }

    fn next(&mut self, now: DateTime<Utc>) -> Option<(&mut EventSchedule, DateTime<Utc>)> {
        self.schedules
            .iter_mut()
            .filter_map(|s| s.schedule.after(&now).next().map(|u| (s, u)))
            .filter(|(s, u)| s.last_sent < *u)
            .min_by_key(|(_s, u)| *u)
    }

    fn event_ready(&mut self) -> Result<Option<(&mut EventSchedule, DateTime<Utc>)>, Error> {
        let resolution = TimeDelta::from_std(self.resolution)?;

        #[cfg(test)]
        let now = mock_chrono::Utc::now();
        #[cfg(not(test))]
        let now = Utc::now();

        if let Some((event_schedule, time)) = self.next(now - resolution) {
            if time < now + resolution {
                return Ok(Some((event_schedule, time)));
            }
        }
        Ok(None)
    }

    async fn run_internal(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<(), Error> {
        info!("start scheduler");

        let mut interval = time::interval(self.resolution);

        loop {
            interval.tick().await;

            if let Some((schedule, time)) = self.event_ready()? {
                info!("schedule triggered: {}", time);
                broadcast.send(Event::TextMessage {
                    user: SYSTEM_USER_NAME.to_string(),
                    message: schedule.message.clone(),
                })?;
                schedule.last_sent = time;
            }
        }
    }
}

#[async_trait]
impl EventComponent for Scheduler {
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| events::Error::Component(format!("scheduler: {}", e)))
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

    struct TemporaryPath {
        path: String,
    }

    impl TemporaryPath {
        fn new() -> Self {
            Self {
                path: String::from("/tmp/tsuki_test_scheduler.json"),
            }
        }
    }

    impl Drop for TemporaryPath {
        fn drop(&mut self) {
            if std::path::Path::new(&self.path).exists() {
                std::fs::remove_file(&self.path).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn daily() {
        let path = TemporaryPath::new();
        let repo = Repository::new(&path.path, false).unwrap();
        let mut scheduler = Scheduler::new(Arc::new(RwLock::new(repo)), Duration::from_secs(60))
            .await
            .unwrap();
        scheduler
            .register(String::from("0 30 19 * * *"), String::from("foo"))
            .await
            .unwrap();
        mock_chrono::set_timestamp(19, 29, 0);
        assert_eq!(scheduler.event_ready().unwrap().is_some(), false);
        mock_chrono::set_timestamp(19, 29, 1);
        assert_eq!(scheduler.event_ready().unwrap().is_some(), true);
        mock_chrono::set_timestamp(19, 30, 0);
        assert_eq!(scheduler.event_ready().unwrap().is_some(), true);
        mock_chrono::set_timestamp(19, 30, 59);
        assert_eq!(scheduler.event_ready().unwrap().is_some(), true);
        mock_chrono::set_timestamp(19, 31, 0);
        assert_eq!(scheduler.event_ready().unwrap().is_some(), false);
    }

    #[tokio::test]
    async fn no_duplicate() {
        let path = TemporaryPath::new();
        let repo = Repository::new(&path.path, false).unwrap();
        let mut scheduler = Scheduler::new(Arc::new(RwLock::new(repo)), Duration::from_secs(60))
            .await
            .unwrap();
        scheduler
            .register(String::from("0 0 20,21 * * *"), String::from("foo"))
            .await
            .unwrap();
        mock_chrono::set_timestamp(20, 0, 0);
        let ready = scheduler.event_ready().unwrap();
        assert_eq!(ready.is_some(), true);
        scheduler.schedules[0].last_sent = ready.unwrap().1;
        assert_eq!(scheduler.event_ready().unwrap().is_some(), false);
        mock_chrono::set_timestamp(20, 59, 0);
        assert_eq!(scheduler.event_ready().unwrap().is_some(), false);
        mock_chrono::set_timestamp(21, 00, 0);
        assert_eq!(scheduler.event_ready().unwrap().is_some(), true);
    }
}
