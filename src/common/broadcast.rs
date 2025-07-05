use anyhow::Result;
use tokio::sync::broadcast::{self, Receiver, Sender};
use uuid::Uuid;

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

    pub fn send(&self, payload: T) -> Result<()> {
        match self.sender.send(Envelope {
            sender_id: self.id,
            payload,
        }) {
            Ok(_) => Ok(()),
            Err(_) => anyhow::bail!("Failed to send event: No active receivers"),
        }
    }

    pub async fn recv(&mut self) -> Result<T> {
        loop {
            match self.receiver.recv().await {
                Ok(env) if env.sender_id != self.id => return Ok(env.payload),
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    anyhow::bail!("Failed to receive event: Closed")
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    anyhow::bail!("Failed to receive event: Lagged {}", n)
                }
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
