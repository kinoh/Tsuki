use fcm::{
    message::{Message, Notification, Target},
    FcmClient,
};
use thiserror::Error;
use tracing::debug;

#[derive(Error, Debug)]
pub enum Error {
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

pub struct MessageSender {
    client: FcmClient,
}

impl MessageSender {
    pub async fn new() -> Result<Self, Error> {
        let client = FcmClient::builder()
            // Comment to use GOOGLE_APPLICATION_CREDENTIALS environment
            // variable. The variable can also be defined in .env file.
            .service_account_key_json_string(include_str!("../service_account_key.json"))
            .build()
            .await?;
        Ok(Self { client })
    }

    pub async fn send(&self, title: &str, topic: &str, content: &str) -> Result<(), Error> {
        let message = Message {
            data: None,
            notification: Some(Notification {
                title: Some(String::from(title)),
                body: Some(String::from(content)),
                image: None,
            }),
            target: Target::Topic(String::from(topic)),
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
}
