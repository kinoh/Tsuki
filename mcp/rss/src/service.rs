use std::cmp::Ordering;

use chrono::{DateTime, TimeZone, Utc};
use feed_rs::model::Entry;
use reqwest::Client;
use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde::Deserialize;
use tokio::task;
use url::Url;

use crate::config::{ConfigError, RssConfig};

const MAX_DESCRIPTION_CHARS: usize = 280;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetArticlesRequest {
    #[schemars(description = "RFC3339 timestamp to filter entries (inclusive)")]
    pub since: Option<String>,
    #[schemars(description = "Maximum number of articles to return (default 20)")]
    pub n: Option<usize>,
}

#[derive(Debug, Clone)]
struct Article {
    title: String,
    url: String,
    published: Option<DateTime<Utc>>,
    description: String,
}

#[derive(Clone)]
pub struct RssService {
    tool_router: ToolRouter<Self>,
    config: RssConfig,
    client: Client,
}

impl RssService {
    pub async fn from_env() -> Result<Self, ErrorData> {
        let config = RssConfig::from_env().await.map_err(map_config_error)?;
        Self::new(config)
    }

    pub fn new(config: RssConfig) -> Result<Self, ErrorData> {
        let client = Client::builder()
            .user_agent(format!(
                "{} ({})",
                concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
                "https://github.com/kinoh/tsuki"
            ))
            .connect_timeout(config.feed_timeout)
            .timeout(config.feed_timeout)
            .build()
            .map_err(|e| {
                ErrorData::internal_error(
                    "Error: fetch: upstream request failed",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;

        Ok(Self {
            tool_router: Self::tool_router(),
            config,
            client,
        })
    }

    fn parse_since(&self, since: &str) -> Result<DateTime<Utc>, ErrorData> {
        DateTime::parse_from_rfc3339(since)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|_| ErrorData::invalid_params("Error: since: invalid timestamp", None))
    }

    async fn fetch_feed(&self, url: &Url) -> Result<Vec<Article>, ErrorData> {
        let response = self.client.get(url.clone()).send().await.map_err(|e| {
            ErrorData::internal_error(
                "Error: fetch: upstream request failed",
                Some(json!({"url": url, "reason": e.to_string()})),
            )
        })?;

        if !response.status().is_success() {
            return Err(ErrorData::internal_error(
                "Error: fetch: upstream request failed",
                Some(json!({"url": url, "status": response.status().as_u16()})),
            ));
        }

        let bytes = response.bytes().await.map_err(|e| {
            ErrorData::internal_error(
                "Error: fetch: upstream request failed",
                Some(json!({"url": url, "reason": e.to_string()})),
            )
        })?;

        let feed = task::spawn_blocking(move || feed_rs::parser::parse(&bytes[..]))
            .await
            .map_err(|e| {
                ErrorData::internal_error(
                    "Error: fetch: upstream request failed",
                    Some(json!({"url": url, "reason": e.to_string()})),
                )
            })?
            .map_err(|e| {
                ErrorData::internal_error(
                    "Error: fetch: upstream request failed",
                    Some(json!({"url": url, "reason": e.to_string()})),
                )
            })?;

        Ok(feed.entries.into_iter().map(entry_to_article).collect())
    }

    fn format_articles(&self, mut articles: Vec<Article>, limit: usize) -> String {
        articles.sort_by(|a, b| match (&a.published, &b.published) {
            (Some(ap), Some(bp)) => bp.cmp(ap),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        });

        let limited = articles.into_iter().take(limit);
        let mut lines = Vec::new();
        let rows: Vec<String> = limited
            .map(|a| {
                format!(
                    "  {},{},{},{}",
                    sanitize(&a.title),
                    sanitize(&a.url),
                    a.published
                        .map(|dt| self.config.tz.from_utc_datetime(&dt.naive_utc()).to_rfc3339())
                        .unwrap_or_default(),
                    sanitize(&a.description)
                )
            })
            .collect();

        lines.push(format!(
            "articles[{}]{{title,url,published_at,description}}:",
            rows.len()
        ));
        lines.extend(rows);
        lines.join("\n")
    }

    async fn collect_articles(&self, since: Option<DateTime<Utc>>) -> Result<Vec<Article>, ErrorData> {
        let mut all = Vec::new();
        for url in &self.config.feeds {
            let mut entries = self.fetch_feed(url).await?;
            if let Some(since) = since {
                entries.retain(|a| match a.published {
                    Some(p) => p.with_timezone(&Utc) >= since,
                    None => false,
                });
            }
            all.extend(entries);
        }
        Ok(all)
    }
}

#[tool_router]
impl RssService {
    #[tool]
    pub async fn get_articles(
        &self,
        Parameters(request): Parameters<GetArticlesRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if self.config.feeds.is_empty() {
            return Err(ErrorData::invalid_params(
                "Error: config: feeds not configured",
                None,
            ));
        }

        let since = match &request.since {
            Some(s) => Some(self.parse_since(s)?),
            None => None,
        };

        let limit = request.n.unwrap_or(20);

        let articles = self.collect_articles(since).await?;
        let text = self.format_articles(articles, limit);

        Ok(CallToolResult {
            content: vec![Content::text(text)],
            structured_content: None,
            is_error: Some(false),
            meta: None,
        })
    }
}

#[tool_handler]
impl ServerHandler for RssService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Simple RSS MCP server that merges configured feeds and returns TOON articles via get_articles.".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: env!("CARGO_CRATE_NAME").to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

fn entry_to_article(entry: Entry) -> Article {
    let title = entry.title.map(|t| t.content).unwrap_or_default();
    let url = entry
        .links
        .first()
        .map(|l| l.href.clone())
        .unwrap_or_default();
    let published = entry.published;
    let raw_description = entry
        .summary
        .as_ref()
        .map(|s| s.content.clone())
        .or_else(|| entry.content.as_ref().and_then(|c| c.body.clone()))
        .unwrap_or_default();
    let description = clean_description(&raw_description);

    Article {
        title,
        url,
        published,
        description,
    }
}

fn sanitize(value: &str) -> String {
    value
        .replace('\n', " ")
        .replace('\r', " ")
        .replace(',', ";")
        .trim()
        .to_string()
}

fn clean_description(raw: &str) -> String {
    let no_tags = strip_tags(raw);
    let normalized = normalize_whitespace(&no_tags);
    truncate(&normalized, MAX_DESCRIPTION_CHARS)
}

fn strip_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn normalize_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(raw: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in raw.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn map_config_error(err: ConfigError) -> ErrorData {
    match err {
        ConfigError::Missing(_) | ConfigError::Invalid(_) | ConfigError::Yaml(_) => {
            ErrorData::invalid_params(
                "Error: config: feeds not configured",
                Some(json!({"reason": err.to_string()})),
            )
        }
        ConfigError::Io(e) => ErrorData::internal_error(
            "Error: config: feeds not configured",
            Some(json!({"reason": e.to_string()})),
        ),
    }
}
