use async_trait::async_trait;
use tracing::{debug, info};
use anyhow::Result;

use crate::{
    adapter::fcm::MessageSender,
    common::{
        broadcast::IdentifiedBroadcast,
        chat::Modality,
        events::{Event, EventComponent},
    },
};

pub struct Notifier {
    client: MessageSender,
}

impl Notifier {
    pub async fn new() -> Result<Self> {
        let client = MessageSender::new().await?;
        Ok(Self { client })
    }

    async fn notify(&self, content: &str) -> Result<()> {
        debug!(message = content, "notify");
        self.client.send("New message", "message", content).await?;
        Ok(())
    }

    async fn run_internal(
        &mut self,
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<()> {
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
    async fn run(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| anyhow::anyhow!("notifier: {}", e))
    }
}
