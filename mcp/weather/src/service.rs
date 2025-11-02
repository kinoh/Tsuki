use regex::{Regex, RegexBuilder};
use reqwest::Client;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router,
};
use serde::Deserialize;
use std::fmt::Write;
use url::Url;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetWeatherRequest {}

#[derive(Clone)]
pub struct WeatherService {
    tool_router: ToolRouter<Self>,
    client: Client,
    location_path: String,
    location_regex: Regex,
    forecast_regex: Regex,
}

impl WeatherService {
    pub fn new(location_path: String) -> Self {
        let normalized = normalize_location_path(location_path);

        Self {
            tool_router: Self::tool_router(),
            client: Client::new(),
            location_path: normalized,
            location_regex: Regex::new(r"(?m)^.*?の天気.+発表").expect("valid location regex"),
            forecast_regex: RegexBuilder::new(
                r"(今日.{0,10}月\d+日.*?最大風速.*?\n+)(明日.{0,10}月\d+日.*?最大風速.*?\n+)",
            )
            .dot_matches_new_line(true)
            .build()
            .expect("valid forecast regex"),
        }
    }

    fn forecast_url(&self) -> Result<Url, ErrorData> {
        Url::parse(&format!("https://tenki.jp/forecast/{}", self.location_path)).map_err(|e| {
            ErrorData::internal_error(
                "Error: failed to parse",
                Some(json!({"reason": format!("invalid forecast URL: {}", e)})),
            )
        })
    }

    async fn ensure_allowed_by_robots(&self, url: &Url) -> Result<(), ErrorData> {
        let mut robots_url = url.clone();
        robots_url.set_path("/robots.txt");
        robots_url.set_query(None);
        robots_url.set_fragment(None);

        let response = self.client.get(robots_url).send().await.map_err(|e| {
            ErrorData::invalid_params(
                "Error: disallowed by robots.txt",
                Some(json!({"reason": format!("failed to fetch robots.txt: {}", e)})),
            )
        })?;

        if !response.status().is_success() {
            return Err(ErrorData::invalid_params(
                "Error: disallowed by robots.txt",
                Some(
                    json!({"reason": format!("robots.txt returned status {}", response.status())}),
                ),
            ));
        }

        let robots_txt = response.text().await.map_err(|e| {
            ErrorData::invalid_params(
                "Error: disallowed by robots.txt",
                Some(json!({"reason": format!("failed to read robots.txt: {}", e)})),
            )
        })?;

        self.validate_robots_rules(&robots_txt, url.path())
    }

    async fn fetch_forecast_markdown(&self) -> Result<String, ErrorData> {
        let url = self.forecast_url()?;
        self.ensure_allowed_by_robots(&url).await?;

        let response = self.client.get(url.clone()).send().await.map_err(|e| {
            ErrorData::internal_error(
                "Error: request failed",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        if !response.status().is_success() {
            return Err(ErrorData::internal_error(
                "Error: request failed",
                Some(json!({"status": response.status().as_u16()})),
            ));
        }

        let html = response.text().await.map_err(|e| {
            ErrorData::internal_error(
                "Error: request failed",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        Ok(html2text::from_read(html.as_bytes(), 80))
    }

    fn compose_forecast(&self, markdown: &str) -> Result<String, ErrorData> {
        let location = self
            .location_regex
            .find(markdown)
            .map(|m| m.as_str().trim().to_string());

        let forecast = self.forecast_regex.captures(markdown).map(|caps| {
            let mut buffer = String::new();
            if let Some(today) = caps.get(1) {
                writeln!(&mut buffer, "{}", today.as_str().trim()).ok();
            }
            if let Some(tomorrow) = caps.get(2) {
                writeln!(&mut buffer, "{}", tomorrow.as_str().trim()).ok();
            }
            buffer.trim().to_string()
        });

        match (location, forecast) {
            (Some(loc), Some(data)) if !loc.is_empty() && !data.is_empty() => {
                Ok(format!("{}\n{}", loc, data))
            }
            _ => Err(ErrorData::internal_error("Error: failed to parse", None)),
        }
    }

    fn validate_robots_rules(&self, robots_txt: &str, path: &str) -> Result<(), ErrorData> {
        for rule in robots_txt.lines().filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.to_lowercase().starts_with("disallow:") {
                trimmed
                    .split_once(':')
                    .map(|(_, value)| value.trim())
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_string())
            } else {
                None
            }
        }) {
            if is_path_disallowed(&rule, path).map_err(|e| {
                ErrorData::invalid_params(
                    "Error: disallowed by robots.txt",
                    Some(json!({"reason": e})),
                )
            })? {
                return Err(ErrorData::invalid_params(
                    "Error: disallowed by robots.txt",
                    Some(json!({"rule": rule, "path": path})),
                ));
            }
        }

        Ok(())
    }
}

#[tool_router]
impl WeatherService {
    #[tool(
        description = "Retrieves a Markdown-formatted weather forecast for the configured location"
    )]
    pub async fn get_weather(
        &self,
        _params: Parameters<GetWeatherRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let markdown = self.fetch_forecast_markdown().await?;
        let forecast = self.compose_forecast(&markdown)?;

        Ok(CallToolResult {
            content: vec![Content::text(forecast)],
            structured_content: None,
            is_error: Some(false),
            meta: None,
        })
    }
}

#[tool_handler]
impl ServerHandler for WeatherService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Weather MCP server that returns today's and tomorrow's forecast for a fixed tenki.jp location. Use the get_weather tool to retrieve the Markdown summary.".into()),
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

fn normalize_location_path(path: String) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{}/", trimmed)
    }
}

fn is_path_disallowed(rule: &str, path: &str) -> Result<bool, String> {
    let mut pattern = regex::escape(rule);
    pattern = pattern.replace(r"\*", ".*");

    if rule.ends_with('$') {
        if let Some(stripped) = pattern.strip_suffix(r"\$") {
            pattern = format!("^{}$", stripped);
        } else {
            pattern = format!("^{}$", pattern);
        }
    } else {
        pattern = format!("^{}.*", pattern);
    }

    let regex = Regex::new(&pattern).map_err(|e| format!("invalid robots rule regex: {}", e))?;
    Ok(regex.is_match(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> WeatherService {
        WeatherService::new("test/".into())
    }

    #[test]
    fn disallow_simple_rule_blocks_matching_paths() {
        let srv = service();
        let robots = "User-agent: *\nDisallow: /private/\n";

        assert!(srv.validate_robots_rules(robots, "/private/data").is_err());
        assert!(srv.validate_robots_rules(robots, "/public/data").is_ok());
    }

    #[test]
    fn disallow_wildcard_rule_blocks_all_forecast_variants() {
        let srv = service();
        let robots = "User-agent: *\nDisallow: /forecast/*\n";

        assert!(srv.validate_robots_rules(robots, "/forecast/abc").is_err());
        assert!(srv.validate_robots_rules(robots, "/forecast/").is_err());
        assert!(srv.validate_robots_rules(robots, "/other/abc").is_ok());
    }

    #[test]
    fn disallow_dollar_rule_matches_only_exact_suffix() {
        let srv = service();
        let robots = "User-agent: *\nDisallow: /*.gif$\n";

        assert!(srv.validate_robots_rules(robots, "/index.gif").is_err());
        assert!(srv.validate_robots_rules(robots, "/index.gift").is_ok());
    }

    #[test]
    fn empty_disallow_allows_access() {
        let srv = service();
        let robots = "User-agent: *\nDisallow: \n";

        assert!(srv.validate_robots_rules(robots, "/any/path").is_ok());
    }

    #[tokio::test]
    async fn robots_txt_fetch_failure_returns_error() {
        let srv = service();
        let url = Url::parse("http://127.0.0.1:65535/any/path").unwrap();

        let result = srv.ensure_allowed_by_robots(&url).await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.message, "Error: disallowed by robots.txt");
    }
}
