use thiserror::Error;
use tokio::sync::broadcast::{self, Receiver, Sender};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to receive event: Closed")]
    ReceiveClosed,
    #[error("Failed to receive event: Lagged {0}")]
    ReceiveLagged(u64),
    #[error("Failed to send event: No active receivers")]
    Send,
}

#[derive(Clone, Debug)]
struct Envelope<T> {
    sender_id: Uuid,
    payload: T,
}

pub struct IdentifiedBroadcast<T> {
    id: Uuid,
    sender: Sender<Envelope<T>>,
    receiver: Receiver<Envelope<T>>,
}

impl<T: Clone> IdentifiedBroadcast<T> {
    pub fn new(capacity: usize) -> Self {
        let id = Uuid::new_v4();
        let (sender, receiver) = broadcast::channel(capacity);

        Self {
            id,
            sender,
            receiver,
        }
    }

    pub fn participate(&self) -> Self {
        let id = Uuid::new_v4();

        Self {
            id,
            sender: self.sender.clone(),
            receiver: self.sender.subscribe(),
        }
    }

    pub fn send(&self, payload: T) -> Result<(), Error> {
        match self.sender.send(Envelope {
            sender_id: self.id,
            payload,
        }) {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::Send),
        }
    }

    pub async fn recv(&mut self) -> Result<T, Error> {
        loop {
            match self.receiver.recv().await {
                Ok(env) if env.sender_id != self.id => return Ok(env.payload),
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Closed) => return Err(Error::ReceiveClosed),
                Err(broadcast::error::RecvError::Lagged(n)) => return Err(Error::ReceiveLagged(n)),
            }
        }
    }
}

impl<T> Clone for IdentifiedBroadcast<T> {
    fn clone(&self) -> Self {
        IdentifiedBroadcast {
            id: self.id,
            sender: self.sender.clone(),
            receiver: self.sender.subscribe(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.id = source.id;
        self.sender = source.sender.clone();
        self.receiver = source.sender.subscribe();
    }
}
