use std::fmt::Debug;

use async_trait::async_trait;
use fcm::{
    message::{Message, Notification, Target},
    FcmClient,
};
use reqwest::Client;
use thiserror::Error;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tracing::{debug, info, warn};

use crate::events::{self, Event, EventComponent};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to receive event: {0}")]
    ReceiveEvent(#[from] broadcast::error::RecvError),
    #[error("Request error status: {0}")]
    HttpRequest(u16),
    #[error("FCM error {0}")]
    Fcm(String),
}

impl From<fcm::FcmClientError> for Error {
    fn from(value: fcm::FcmClientError) -> Self {
        Error::Fcm(format!("client: {:?}", value))
    }
}

impl From<fcm::response::FcmResponseError> for Error {
    fn from(value: fcm::response::FcmResponseError) -> Self {
        Error::Fcm(format!("response: {:?}", value))
    }
}

pub struct Notifier {
    client: FcmClient,
}

impl Notifier {
    pub async fn new() -> Result<Self, Error> {
        let client = fcm::FcmClient::builder()
            // Comment to use GOOGLE_APPLICATION_CREDENTIALS environment
            // variable. The variable can also be defined in .env file.
            .service_account_key_json_string(include_str!("service_account_key.json"))
            .build()
            .await?;
        Ok(Self { client })
    }

    async fn notify(&self, content: &str) -> Result<(), Error> {
        debug!(message = content, "notify");

        let message = Message {
            data: None,
            notification: Some(Notification {
                title: Some("Tsuki".to_string()),
                body: Some(content.to_string()),
                image: None,
            }),
            target: Target::Topic("message".to_string()),
            android: None,
            webpush: None,
            apns: None,
            fcm_options: None,
        };

        let response = self.client.send(message).await?;

        debug!(response = format!("{:?}", response), "request sent");

        let status = response.http_status_code();
        if status >= 400 {
            return Err(Error::HttpRequest(status));
        }

        if let Some(error) = response.error() {
            let error = error.into();
            return Err(error);
        }

        Ok(())
    }

    async fn run_internal(
        &mut self,
        _sender: Sender<Event>,
        mut receiver: Receiver<Event>,
    ) -> Result<(), Error> {
        info!("start notifier");

        loop {
            let event = receiver.recv().await?;
            let serialized = format!("{}", event);
            info!(event = serialized);
        }
    }
}

#[async_trait]
impl EventComponent for Notifier {
    async fn run(&mut self, sender: Sender<Event>) -> Result<(), crate::events::Error> {
        let receiver = sender.subscribe();
        self.run_internal(sender, receiver)
            .await
            .map_err(|e| events::Error::Component(format!("notifier: {}", e)))
    }
}
