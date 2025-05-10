use std::fmt::Debug;

use async_trait::async_trait;
use thiserror::Error;
use tracing::{debug, info};

use crate::{
    adapter::fcm::MessageSender,
    common::{
        broadcast::{self, IdentifiedBroadcast},
        chat::Modality,
        events::{self, Event, EventComponent},
    },
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("FCM error: {0}")]
    Fcm(#[from] crate::adapter::fcm::Error),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
}

pub struct Notifier {
    client: MessageSender,
}

impl Notifier {
    pub async fn new() -> Result<Self, Error> {
        let client = MessageSender::new().await?;
        Ok(Self { client })
    }

    async fn notify(&self, content: &str) -> Result<(), Error> {
        debug!(message = content, "notify");
        self.client.send("New message", "message", content).await?;
        Ok(())
    }

    async fn run_internal(
        &mut self,
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), Error> {
        info!("start notifier");

        loop {
            let event = broadcast.recv().await?;
            match event {
                Event::AssistantMessage {
                    modality,
                    message,
                    usage: _,
                } => {
                    if modality != Modality::None {
                        self.notify(&message).await?
                    }
                }
                Event::Notify { content } => self.notify(&content).await?,
                _ => (),
            }
        }
    }
}

#[async_trait]
impl EventComponent for Notifier {
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| events::Error::Component(format!("notifier: {}", e)))
    }
}
