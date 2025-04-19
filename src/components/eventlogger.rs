use async_trait::async_trait;
use thiserror::Error;
use tracing::info;

use crate::common::{
    broadcast::{self, IdentifiedBroadcast},
    events::{self, Event, EventComponent},
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
}

pub struct EventLogger {}

impl EventLogger {
    pub fn new() -> Self {
        Self {}
    }

    async fn run_internal(
        &mut self,
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), Error> {
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
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| events::Error::Component(format!("event logger: {}", e)))
    }
}
