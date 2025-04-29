use std::env;

use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum Error {
    #[error("DIFY_SANDBOX_API_KEY not set")]
    MissingApiKey,
    #[error("Invalid response: {0}")]
    HttpRequest(String),
    #[error("Code execution error: code={0}, message={1}, detail={2:?}")]
    CodeExecution(i32, String, Option<String>),
    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),
    #[error("Request error: {0}")]
    Reqwest(#[from] reqwest::Error),
}

#[derive(Deserialize)]
struct SandboxRunResultData {
    error: String,
    stdout: String,
}

#[derive(Deserialize)]
struct SandboxRunResult {
    code: i32,
    message: String,
    data: Option<SandboxRunResultData>,
}

pub struct CodeExecutor {
    endpoint: String,
    api_key: String,
}

impl CodeExecutor {
    pub fn new(dify_sandbox_host: &str) -> Result<Self, Error> {
        let api_key = env::var("DIFY_SANDBOX_API_KEY").map_err(|_| Error::MissingApiKey)?;
        Ok(Self {
            endpoint: format!("http://{}/v1/sandbox/run", dify_sandbox_host),
            api_key,
        })
    }

    pub async fn execute(&self, code: &str) -> Result<String, Error> {
        info!(code = code, "execute");

        let json = serde_json::json!({
            "language": "python3",
            "code": code,
            "enable_network": true,
        });

        let client = Client::new();
        let response = client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("X-Api-Key", &self.api_key)
            .json(&json)
            .send()
            .await
            .map_err(|e| Error::HttpRequest(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            warn!(status = status.as_str(), "request failed");
            return Err(Error::HttpRequest(format!("response code={}", status)));
        }

        let body = response.text().await?;
        info!(body = body, "response");

        let result: SandboxRunResult = serde_json::from_str(&body)?;

        match result.data {
            Some(data) => {
                if data.error.is_empty() {
                    Ok(data.stdout)
                } else {
                    Err(Error::CodeExecution(
                        result.code,
                        result.message,
                        Some(data.error),
                    ))
                }
            }
            None => Err(Error::CodeExecution(result.code, result.message, None)),
        }
    }
}
