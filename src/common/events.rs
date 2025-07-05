use async_trait::async_trait;
use std::{any::type_name, fmt::Display};
use tokio::task::{self, JoinHandle};
use tracing::error;
use anyhow::Result;

use super::{broadcast::IdentifiedBroadcast, chat::Modality};

#[derive(Clone, Debug)]
pub enum Event {
    TextMessage {
        user: String,
        message: String,
    },
    AssistantMessage {
        modality: Modality,
        message: String,
        usage: u32,
    },
    RecognizedSpeech {
        user: String,
        message: String,
    },
    PlayAudio {
        sample_rate: u32,
        audio: Vec<i16>,
    },
    Notify {
        content: String,
    },
    SchedulesUpdated,
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlayAudio { sample_rate, audio } => {
                write!(
                    f,
                    "PlayAudio {{ sample_rate: {}, audio: <{}> }}",
                    sample_rate,
                    audio.len()
                )
            }
            event => {
                write!(f, "{:?}", event)
            }
        }
    }
}

#[async_trait]
pub trait EventComponent {
    async fn run(&mut self, broadcast: IdentifiedBroadcast<Event>) -> Result<()>;
}

pub struct EventSystem {
    broadcast: IdentifiedBroadcast<Event>,
    futures: Vec<Option<JoinHandle<Result<(), anyhow::Error>>>>,
}

impl EventSystem {
    pub fn new(capacity: usize) -> Self {
        EventSystem {
            broadcast: IdentifiedBroadcast::new(capacity),
            futures: Vec::new(),
        }
    }

    pub fn futures(&mut self) -> Vec<JoinHandle<Result<(), anyhow::Error>>> {
        self.futures
            .drain(..)
            .filter_map(|ref mut f| f.take())
            .collect()
    }

    pub fn run<T: EventComponent + Send + 'static>(&mut self, mut component: T) {
        let broadcast = self.broadcast.participate();
        self.futures.push(Some(task::spawn(async move {
            let result = component.run(broadcast).await;
            error!(
                component = type_name::<T>(),
                "component exit: {:?}",
                result.as_ref().err()
            );
            result
        })));
    }
}
