use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::info;

use crate::common::{
    broadcast::IdentifiedBroadcast,
    events::{Event, EventComponent},
};

pub struct EventLogger {}

impl EventLogger {
    pub fn new() -> Self {
        Self {}
    }

    async fn run_internal(&mut self, mut broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        info!("start event logger");

        loop {
            let event = broadcast.recv().await?;
            let serialized = format!("{}", event);
            info!(event = serialized);
        }
    }
}

#[async_trait]
impl EventComponent for EventLogger {
    async fn run(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        self.run_internal(broadcast.participate())
            .await
            .context("event logger")
    }
}
