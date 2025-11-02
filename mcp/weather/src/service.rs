use regex::Regex;
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
use std::time::Duration;
use url::Url;

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct GetWeatherRequest {}

#[derive(Clone)]
pub struct WeatherService {
    tool_router: ToolRouter<Self>,
    client: Client,
    location_path: String,
    location_regex: Regex,
    today_heading_regex: Regex,
    tomorrow_heading_regex: Regex,
}

impl WeatherService {
    pub fn new(location_path: String) -> Self {
        let normalized = normalize_location_path(location_path);
        const USER_AGENT: &str = concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/kinoh/tsuki)"
        );

        Self {
            tool_router: Self::tool_router(),
            client: Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(4))
                .build()
                .expect("reqwest client with user agent"),
            location_path: normalized,
            location_regex: Regex::new(r"(?m)^.*?の天気.+発表").expect("valid location regex"),
            today_heading_regex: Regex::new(r"(?m)^#{0,6}\s*今日[^\n]*")
                .expect("valid today heading regex"),
            tomorrow_heading_regex: Regex::new(r"(?m)^#{0,6}\s*明日[^\n]*")
                .expect("valid tomorrow heading regex"),
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
            .map(|m| m.as_str().trim().to_string())
            .ok_or_else(|| {
                ErrorData::internal_error(
                    "Error: failed to parse",
                    Some(json!({"reason": "location line not found"})),
                )
            })?;

        let today_match = self.today_heading_regex.find(markdown).ok_or_else(|| {
            ErrorData::internal_error(
                "Error: failed to parse",
                Some(json!({"reason": "today section not found"})),
            )
        })?;

        let tomorrow_match = self.tomorrow_heading_regex.find(markdown).ok_or_else(|| {
            ErrorData::internal_error(
                "Error: failed to parse",
                Some(json!({"reason": "tomorrow section not found"})),
            )
        })?;

        if tomorrow_match.start() <= today_match.start() {
            return Err(ErrorData::internal_error(
                "Error: failed to parse",
                Some(json!({"reason": "tomorrow section precedes today section"})),
            ));
        }

        let today_section =
            self.capture_section(markdown, today_match.start(), tomorrow_match.start())?;
        let tomorrow_section =
            self.capture_section(markdown, tomorrow_match.start(), markdown.len())?;

        Ok(format!(
            "{}\n{}\n{}",
            location,
            today_section.trim_end(),
            tomorrow_section.trim_end()
        ))
    }

    fn capture_section(
        &self,
        markdown: &str,
        start: usize,
        end: usize,
    ) -> Result<String, ErrorData> {
        let section_slice = markdown
            .get(start..end)
            .ok_or_else(|| ErrorData::internal_error("Error: failed to parse", None))?;

        if let Some(max_wind_pos) = section_slice.find("最大風速") {
            let remainder = &section_slice[max_wind_pos..];
            let line_end_offset = remainder
                .find('\n')
                .map(|idx| max_wind_pos + idx + 1)
                .unwrap_or(section_slice.len());

            let captured = section_slice[..line_end_offset].trim_end().to_string();

            if captured.is_empty() {
                Err(ErrorData::internal_error(
                    "Error: failed to parse",
                    Some(json!({"reason": "empty section"})),
                ))
            } else {
                Ok(captured)
            }
        } else {
            Err(ErrorData::internal_error(
                "Error: failed to parse",
                Some(json!({"reason": "max wind not found"})),
            ))
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

    #[test]
    fn forecast_extraction_handles_real_markdown() {
        let srv = service();
        let markdown = r#"## 新宿区の天気02日16:00発表

### 今日 11月02日(日)

[日の出]日の出｜06時04分

[日の入]日の入｜16時45分

[曇のち晴]

曇のち晴

*最高*
  19 ℃
  [-2]
*最低*
  12 ℃
  [-1]

────────┬─────┬─────┬─────┬─────
時間    │00-06│06-12│12-18│18-24
────────┼─────┼─────┼─────┼─────
降水確率│---  │---  │20%  │0%
────────┼─────┴─────┴─────┴─────
最大風速│南2m/s
────────┴───────────────────────

### 明日 11月03日(月)

[日の出]日の出｜06時05分

[日の入]日の入｜16時44分

[晴一時雨]

晴一時雨

*最高*
  19℃
  [-1]
*最低*
  11℃
  [-1]

────────┬─────┬─────┬─────┬─────
時間    │00-06│06-12│12-18│18-24
────────┼─────┼─────┼─────┼─────
降水確率│0%   │50%  │10%  │0%
────────┼─────┴─────┴─────┴─────
最大風速│北西7m/s
────────┴───────────────────────

* [時間]16:40現在
* [[温度]16.9℃(前日差:-0.7℃)][1]
"#;

        let forecast = srv
            .compose_forecast(markdown)
            .expect("forecast should parse");
        assert!(forecast.contains("新宿区の天気02日16:00発表"));
        assert!(forecast.contains("### 今日 11月02日(日)"));
        assert!(forecast.contains("最大風速│南2m/s"));
        assert!(forecast.contains("### 明日 11月03日(月)"));
        assert!(forecast.contains("最大風速│北西7m/s"));
    }
}
