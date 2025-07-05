use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

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
    pub fn new(dify_sandbox_host: &str, api_key: &str) -> Result<Self> {
        Ok(Self {
            endpoint: format!("http://{}/v1/sandbox/run", dify_sandbox_host),
            api_key: api_key.to_string(),
        })
    }

    pub async fn execute(&self, code: &str) -> Result<String> {
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
            .context("HTTP request failed")?;

        let status = response.status();
        if !status.is_success() {
            warn!(status = status.as_str(), "request failed");
            anyhow::bail!("HTTP request failed with status: {}", status);
        }

        let body: String = response.text().await?;
        info!(body = body, "response");

        let result: SandboxRunResult = serde_json::from_str(&body)?;

        match result.data {
            Some(data) => {
                if data.error.is_empty() {
                    Ok(data.stdout)
                } else {
                    anyhow::bail!(
                        "Code execution error: code={}, message={}, detail={:?}",
                        result.code,
                        result.message,
                        data.error
                    );
                }
            }
            None => anyhow::bail!(
                "Code execution error: code={}, message={}",
                result.code,
                result.message
            ),
        }
    }
}
