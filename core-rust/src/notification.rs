use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

#[derive(Clone)]
pub struct FcmNotificationSender {
    client: Client,
    project_id: String,
    service_account: ServiceAccountKey,
}

impl FcmNotificationSender {
    pub fn from_env() -> Result<Self, String> {
        let project_id = std::env::var("FCM_PROJECT_ID")
            .map_err(|_| "FCM_PROJECT_ID environment variable is not set".to_string())?;
        let raw_service_account = std::env::var("GCP_SERVICE_ACCOUNT_KEY")
            .map_err(|_| "GCP_SERVICE_ACCOUNT_KEY environment variable is not set".to_string())?;
        let service_account = serde_json::from_str::<ServiceAccountKey>(&raw_service_account)
            .map_err(|err| format!("invalid GCP_SERVICE_ACCOUNT_KEY: {}", err))?;
        if service_account.client_email.trim().is_empty() {
            return Err("service account client_email is missing".to_string());
        }
        if service_account.private_key.trim().is_empty() {
            return Err("service account private_key is missing".to_string());
        }

        Ok(Self {
            client: Client::new(),
            project_id,
            service_account,
        })
    }

    pub async fn send_to_tokens(
        &self,
        tokens: &[String],
        title: &str,
        body: &str,
    ) -> Result<(), String> {
        if tokens.is_empty() {
            return Ok(());
        }

        let access_token = self.access_token().await?;
        for token in tokens {
            if token.trim().is_empty() {
                continue;
            }
            if let Err(err) = self
                .send_single(&access_token, token.trim(), title, body)
                .await
            {
                eprintln!("FCM_SEND_ERROR token={} error={}", token, err);
            }
        }
        Ok(())
    }

    async fn access_token(&self) -> Result<String, String> {
        let token_uri = self
            .service_account
            .token_uri
            .as_deref()
            .unwrap_or(DEFAULT_TOKEN_URI);
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let claims = ServiceAccountClaims {
            iss: self.service_account.client_email.clone(),
            scope: FCM_SCOPE.to_string(),
            aud: token_uri.to_string(),
            iat: now,
            exp: now + 3600,
        };
        let key = EncodingKey::from_rsa_pem(self.service_account.private_key.as_bytes())
            .map_err(|err| format!("failed to parse service account private_key: {}", err))?;
        let assertion = encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|err| format!("failed to sign oauth assertion: {}", err))?;

        let response = self
            .client
            .post(token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|err| format!("failed to request oauth access token: {}", err))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| format!("failed to read oauth response: {}", err))?;
        if !status.is_success() {
            return Err(format!("oauth token request failed status={} body={}", status, text));
        }
        let parsed: TokenResponse = serde_json::from_str(&text)
            .map_err(|err| format!("failed to parse oauth response: {}", err))?;
        Ok(parsed.access_token)
    }

    async fn send_single(
        &self,
        access_token: &str,
        token: &str,
        title: &str,
        body: &str,
    ) -> Result<(), String> {
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );
        let payload = FcmSendRequest {
            message: FcmMessage {
                token: token.to_string(),
                notification: FcmNotification {
                    title: title.to_string(),
                    body: body.to_string(),
                },
            },
        };
        let response = self
            .client
            .post(url)
            .bearer_auth(access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|err| format!("failed to send fcm request: {}", err))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response
            .text()
            .await
            .map_err(|err| format!("failed to read fcm error response: {}", err))?;
        Err(format!("fcm send failed status={} body={}", status, body))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: Option<String>,
}

#[derive(Debug, Serialize)]
struct ServiceAccountClaims {
    iss: String,
    scope: String,
    aud: String,
    iat: i64,
    exp: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Serialize)]
struct FcmSendRequest {
    message: FcmMessage,
}

#[derive(Debug, Serialize)]
struct FcmMessage {
    token: String,
    notification: FcmNotification,
}

#[derive(Debug, Serialize)]
struct FcmNotification {
    title: String,
    body: String,
}
