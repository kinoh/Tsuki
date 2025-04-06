use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::info;

use crate::events::{self, Event, EventComponent};

#[derive(Error, Debug)]
pub enum Error {
    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),
    #[error("Request error: {0}")]
    ReceiveEvent(#[from] broadcast::error::RecvError),
}

pub struct EventLogger {}

impl EventLogger {
    pub fn new() -> Self {
        Self {}
    }

    async fn run_internal(
        &mut self,
        _sender: Sender<Event>,
        mut receiver: Receiver<Event>,
    ) -> Result<(), Error> {
        info!("start event logger");

        loop {
            let event = receiver.recv().await?;
            let serialized = format!("{}", event);
            info!(event = serialized);
        }
    }
}

#[async_trait]
impl EventComponent for EventLogger {
    async fn run(&mut self, sender: Sender<Event>) -> Result<(), crate::events::Error> {
        let receiver = sender.subscribe();
        self.run_internal(sender, receiver)
            .await
            .map_err(|e| events::Error::Component(format!("event logger: {}", e)))
    }
}
