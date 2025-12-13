use std::{env, path::PathBuf, time::Duration};

use chrono_tz::Tz;
use serde::Deserialize;
use thiserror::Error;
use tokio::fs;
use url::Url;

#[derive(Debug, Deserialize)]
struct RssYaml {
    feeds: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RssConfig {
    pub data_dir: PathBuf,
    pub tz: Tz,
    pub feed_timeout: Duration,
    pub feeds: Vec<Url>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Error: config: {0}")]
    Missing(&'static str),
    #[error("Error: config: {0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Error: config: invalid yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

impl RssConfig {
    pub async fn from_env() -> Result<Self, ConfigError> {
        let data_dir = env::var("DATA_DIR").map_err(|_| ConfigError::Missing("DATA_DIR not set"))?;
        let tz_str = env::var("TZ").map_err(|_| ConfigError::Missing("TZ not set"))?;
        let tz = tz_str
            .parse::<Tz>()
            .map_err(|e| ConfigError::Invalid(format!("TZ parse error: {}", e)))?;

        let timeout_secs = env::var("FEED_TIMEOUT_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(2);

        let config_path = PathBuf::from(&data_dir).join("rss.yaml");
        let yaml = fs::read_to_string(&config_path).await?;
        let parsed: RssYaml = serde_yaml::from_str(&yaml)?;
        let feeds = parsed
            .feeds
            .into_iter()
            .map(|raw| {
                Url::parse(&raw).map_err(|e| {
                    ConfigError::Invalid(format!("invalid feed url '{}': {}", raw, e))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        if feeds.is_empty() {
            return Err(ConfigError::Invalid("feeds not configured".into()));
        }

        Ok(Self {
            data_dir: PathBuf::from(data_dir),
            tz,
            feed_timeout: Duration::from_secs(timeout_secs),
            feeds,
        })
    }
}
