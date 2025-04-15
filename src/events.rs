use std::fmt::Display;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::broadcast::{self, Sender};
use tokio::task::{self, JoinHandle};

use crate::messages::Modality;

#[derive(Clone, Debug)]
pub enum Event {
    TextMessage { user: String, message: String },
    SystemMessage { modality: Modality, message: String },
    AssistantMessage { modality: Modality, message: String },
    RecognizedSpeech { user: String, message: String },
    PlayAudio { sample_rate: u32, audio: Vec<i16> },
    Notify { content: String },
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

#[derive(Error, Debug)]
pub enum Error {
    #[error("component error: {0}")]
    Component(String),
    #[error("event receiving error: {0}")]
    EventRecv(#[from] broadcast::error::RecvError),
}

#[async_trait]
pub trait EventComponent {
    async fn run(&mut self, sender: Sender<Event>) -> Result<(), Error>;
}

pub struct EventSystem {
    sender: Sender<Event>,
    futures: Vec<Option<JoinHandle<Result<(), Error>>>>,
}

impl EventSystem {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        EventSystem {
            sender,
            futures: Vec::new(),
        }
    }

    pub fn futures(&mut self) -> Vec<JoinHandle<Result<(), Error>>> {
        self.futures
            .drain(..)
            .filter_map(|ref mut f| f.take())
            .collect()
    }

    pub fn run<T: EventComponent + Send + 'static>(&mut self, mut component: T) {
        let sender = self.sender.clone();
        self.futures.push(Some(task::spawn(
            async move { component.run(sender).await },
        )));
    }
}
