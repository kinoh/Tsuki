use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tokio::select;
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;
use tracing::info;

use crate::common::broadcast::{self, IdentifiedBroadcast};
use crate::common::events::{self, Event, EventComponent};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Signal channel closed")]
    SignalChannelClosed,
    #[error("Component error: {0}")]
    Component(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
}

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

    async fn run_internal(
        &mut self,
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), Error> {
        info!("start interactive adapter");

        let mut internal_broadcast = IdentifiedBroadcast::new(self.capacity);
        let mut fut_component = self.component.run(internal_broadcast.participate());

        loop {
            select! {
                event = broadcast.recv() => {
                    let event = event?;
                    info!("Waiting for signal...");
                    *(self.is_waiting.write().await) = true;
                    self.receiver.recv().await.ok_or(Error::SignalChannelClosed)?;
                    *(self.is_waiting.write().await) = false;
                    internal_broadcast.send(event)?;
                }
                output = internal_broadcast.recv() => {
                    broadcast.send(output?)?;
                }
                result = &mut fut_component => {
                    return result.map_err(|e| Error::Component(e.to_string()));
                }
            }
        }
    }
}

#[async_trait]
impl<T: EventComponent + Send + 'static> EventComponent for InteractiveProxy<T> {
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| events::Error::Component(format!("interactive adapter: {}", e)))
    }
}
