use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::time;
use tracing::{debug, info};

use crate::common::{
    broadcast::{self, IdentifiedBroadcast},
    events::{self, Event, EventComponent},
    messages::Modality,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
}

pub struct Ticker {
    interval: Duration,
}

impl Ticker {
    pub fn new(interval: Duration) -> Self {
        Self { interval }
    }

    async fn run_internal(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<(), Error> {
        info!(interval_secs = self.interval.as_secs_f32(), "start ticker");

        let now = Utc::now();
        let delta = now
            .signed_duration_since(DateTime::UNIX_EPOCH)
            .num_seconds() as u64;
        let interval_secs = self.interval.as_secs();
        let next = (delta / interval_secs + 1) * interval_secs;
        let initial_sleep = next - delta;

        debug!(secs = initial_sleep, "initial sleep");

        time::sleep(Duration::from_secs(initial_sleep)).await;

        let mut interval = time::interval(self.interval);

        loop {
            interval.tick().await;
            let now = Utc::now();
            let minutes = self.interval.as_secs_f32() / 60f32;
            debug!("tick");
            broadcast.send(Event::SystemMessage {
                modality: Modality::Tick,
                message: format!(
                    "{} ({}m interval)",
                    now.format("%Y-%m-%d %H:%M:%S"),
                    minutes
                ),
            })?;
        }
    }
}

#[async_trait]
impl EventComponent for Ticker {
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| events::Error::Component(format!("ticker: {}", e)))
    }
}
