use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::select;
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;
use tracing::info;

use crate::common::broadcast::IdentifiedBroadcast;
use crate::common::events::{Event, EventComponent};

pub enum Signal {
    Continue,
}

pub struct InteractiveProxy<T: EventComponent + Send + 'static> {
    capacity: usize,
    receiver: Receiver<Signal>,
    component: T,
    is_waiting: Arc<RwLock<bool>>,
}

impl<T: EventComponent + Send + 'static> InteractiveProxy<T> {
    pub fn new(capacity: usize, receiver: Receiver<Signal>, component: T) -> Self {
        Self {
            capacity,
            receiver,
            component,
            is_waiting: Arc::new(RwLock::new(false)),
        }
    }

    pub fn watch(&self) -> Arc<RwLock<bool>> {
        self.is_waiting.clone()
    }

    async fn run_internal(&mut self, mut broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        info!("start interactive adapter");

        let mut internal_broadcast = IdentifiedBroadcast::new(self.capacity);
        let mut fut_component = self.component.run(internal_broadcast.participate());

        loop {
            select! {
                event = broadcast.recv() => {
                    let event = event?;
                    info!("Waiting for signal...");
                    *(self.is_waiting.write().await) = true;
                    self.receiver.recv().await.context("Signal channel closed")?;
                    *(self.is_waiting.write().await) = false;
                    internal_broadcast.send(event)?;
                }
                output = internal_broadcast.recv() => {
                    broadcast.send(output?)?;
                }
                result = &mut fut_component => {
                    return result.context("Component error");
                }
            }
        }
    }
}

#[async_trait]
impl<T: EventComponent + Send + 'static> EventComponent for InteractiveProxy<T> {
    async fn run(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<()> {
        self.run_internal(broadcast.participate())
            .await
            .context("interactive adapter")
    }
}
