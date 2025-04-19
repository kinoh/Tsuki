use std::fmt::Debug;

use async_trait::async_trait;
use fcm::{
    message::{Message, Notification, Target},
    FcmClient,
};
use thiserror::Error;
use tracing::{debug, info};

use crate::common::{
    broadcast::{self, IdentifiedBroadcast},
    events::{self, Event, EventComponent},
    messages::Modality,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Request error status: {0}")]
    HttpRequest(u16),
    #[error("FCM error {0}")]
    Fcm(String),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
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
            .service_account_key_json_string(include_str!("../service_account_key.json"))
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
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), Error> {
        info!("start notifier");

        loop {
            let event = broadcast.recv().await?;
            match event {
                Event::AssistantMessage { modality, message } => {
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
