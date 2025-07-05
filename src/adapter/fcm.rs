use fcm::{
    message::{Message, Notification, Target},
    FcmClient,
};
use anyhow::Result;
use tracing::debug;

pub struct MessageSender {
    client: FcmClient,
}

impl MessageSender {
    pub async fn new() -> Result<Self> {
        let client = FcmClient::builder()
            // Comment to use GOOGLE_APPLICATION_CREDENTIALS environment
            // variable. The variable can also be defined in .env file.
            .service_account_key_json_string(include_str!("../service_account_key.json"))
            .build()
            .await?;
        Ok(Self { client })
    }

    pub async fn send(&self, title: &str, topic: &str, content: &str) -> Result<()> {
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
            anyhow::bail!("Request error status: {}", status);
        }

        if let Some(error) = response.error() {
            anyhow::bail!("FCM error: {:?}", error);
        }

        Ok(())
    }
}
