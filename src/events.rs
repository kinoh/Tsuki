use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::task::{self, JoinHandle};

#[derive(Clone, Debug)]
pub enum Event {
    RecognizedSpeech { user: String, message: String },
    AssistantSpeech { message: String },
    PlayAudio { sample_rate: u32, audio: Vec<i16> },
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
    async fn run(
        &mut self,
        sender: Sender<Event>,
        receiver: &mut Receiver<Event>,
    ) -> Result<(), Error>;
}

pub struct EventSystem {
    sender: Sender<Event>,
}

impl EventSystem {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        EventSystem { sender }
    }

    pub async fn run<T: EventComponent + Send + 'static>(
        &self,
        mut component: T,
    ) -> JoinHandle<Result<(), Error>> {
        let mut receiver = self.sender.subscribe();
        let sender = self.sender.clone();
        task::spawn(async move { component.run(sender, &mut receiver).await })
    }
}
