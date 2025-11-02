use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use reqwest::{Client, Url};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router,
};
use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct MetricQuery {
    pub name: String,
    pub expression: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetMetricRequest {
    #[serde(default)]
    pub at: Option<String>,
}

#[derive(Debug)]
struct ToonRow {
    name: String,
    timestamp: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct PrometheusResponse {
    status: String,
    data: Option<PrometheusData>,
    #[serde(rename = "errorType")]
    error_type: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PrometheusData {
    #[serde(default)]
    result: Vec<PrometheusResult>,
}

#[derive(Debug, Deserialize)]
struct PrometheusResult {
    #[serde(default)]
    metric: HashMap<String, String>,
    value: (String, String),
}

#[derive(Debug, Clone)]
pub struct MetricsService {
    tool_router: ToolRouter<Self>,
    client: Client,
    query_url: Url,
    timezone: Tz,
    queries: Vec<MetricQuery>,
    auth: Option<(String, String)>,
}

impl MetricsService {
    pub fn new(
        client: Client,
        query_url: Url,
        timezone: Tz,
        queries: Vec<MetricQuery>,
        auth: Option<(String, String)>,
    ) -> Result<Self, ErrorData> {
        if queries.is_empty() {
            return Err(ErrorData::internal_error(
                "No queries were configured via METRICS_QUERIES",
                None,
            ));
        }

        Ok(Self {
            tool_router: Self::tool_router(),
            client,
            query_url,
            timezone,
            queries,
            auth,
        })
    }

    pub fn build_client(timeout: Duration) -> Result<Client, ErrorData> {
        Client::builder().timeout(timeout).build().map_err(|err| {
            ErrorData::internal_error(
                "Failed to create HTTP client",
                Some(json!({
                    "reason": err.to_string(),
                })),
            )
        })
    }

    pub fn build_query_url(base: &Url) -> Result<Url, ErrorData> {
        base.join("/api/v1/query").map_err(|err| {
            ErrorData::invalid_params(
                "Error: PROMETHEUS_BASE_URL: invalid base URL",
                Some(json!({
                    "reason": err.to_string(),
                })),
            )
        })
    }

    pub fn parse_queries(raw: &str) -> Vec<MetricQuery> {
        raw.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    return None;
                }
                let (name, expression) = trimmed.split_once('=')?;
                let name = name.trim();
                let expression = expression.trim();
                if name.is_empty() || expression.is_empty() {
                    eprintln!(
                        "Skipping invalid METRICS_QUERIES entry (missing name or expression): {}",
                        line
                    );
                    return None;
                }
                Some(MetricQuery {
                    name: name.to_string(),
                    expression: expression.to_string(),
                })
            })
            .collect()
    }

    async fn query_metric(
        &self,
        query: &MetricQuery,
        at: Option<DateTime<Utc>>,
    ) -> Result<ToonRow, ErrorData> {
        let mut request = self
            .client
            .get(self.query_url.clone())
            .query(&[("query", query.expression.as_str())]);

        if let Some((ref username, ref password)) = self.auth {
            request = request.basic_auth(username, Some(password));
        }

        if let Some(at) = at {
            request = request.query(&[("time", at.to_rfc3339())]);
        }

        let response = request.send().await.map_err(|err| {
            ErrorData::internal_error(
                "Error: upstream: request failed",
                Some(json!({
                    "query": query.name,
                    "reason": err.to_string(),
                })),
            )
        })?;

        let status = response.status();
        let body = response.text().await.map_err(|err| {
            ErrorData::internal_error(
                "Error: upstream: failed to read body",
                Some(json!({
                    "query": query.name,
                    "reason": err.to_string(),
                })),
            )
        })?;

        if !status.is_success() {
            return Err(ErrorData::internal_error(
                "Error: upstream: non-success status",
                Some(json!({
                    "status": status.as_u16(),
                    "body": body,
                    "query": query.name,
                })),
            ));
        }

        let parsed: PrometheusResponse = serde_json::from_str(&body).map_err(|err| {
            ErrorData::internal_error(
                "Error: upstream: failed to parse response",
                Some(json!({
                    "query": query.name,
                    "reason": err.to_string(),
                })),
            )
        })?;

        if parsed.status != "success" {
            let reason = parsed
                .error
                .or(parsed.error_type)
                .unwrap_or_else(|| "unknown error".to_string());
            return Err(ErrorData::internal_error(
                "Error: upstream: query failed",
                Some(json!({
                    "query": query.name,
                    "reason": reason,
                })),
            ));
        }

        let data = parsed.data.ok_or_else(|| {
            ErrorData::internal_error(
                "Error: upstream: missing data section",
                Some(json!({
                    "query": query.name,
                })),
            )
        })?;

        let sample = data.result.first().ok_or_else(|| {
            ErrorData::internal_error(
                "Error: upstream: empty result",
                Some(json!({
                    "query": query.name,
                })),
            )
        })?;

        let (timestamp_str, value_str) = &sample.value;
        let metric_name = sample
            .metric
            .get("__name__")
            .cloned()
            .unwrap_or_else(|| query.name.clone());
        let timestamp = parse_timestamp(timestamp_str).map_err(|err| {
            ErrorData::internal_error(
                "Error: upstream: invalid timestamp",
                Some(json!({
                    "query": query.name,
                    "reason": err,
                })),
            )
        })?;

        let local_time = timestamp.with_timezone(&self.timezone);

        Ok(ToonRow {
            name: metric_name,
            timestamp: local_time.to_rfc3339(),
            value: value_str.clone(),
        })
    }

    fn build_toon(rows: &[ToonRow]) -> String {
        let mut buffer = String::from("results[1]{name,timestamp,value}:\n");
        for row in rows {
            // Use the original value string to preserve formatting like NaN/Inf.
            let _ = std::fmt::Write::write_fmt(
                &mut buffer,
                format_args!("  {},{},{}\n", row.name, row.timestamp, row.value),
            );
        }
        buffer
    }
}

fn parse_timestamp(raw: &str) -> Result<DateTime<Utc>, String> {
    let raw = raw.trim();
    let ts = raw
        .parse::<f64>()
        .map_err(|err| format!("failed to parse timestamp '{}': {}", raw, err))?;

    let mut seconds = ts.trunc() as i64;
    let mut nanos = ((ts - seconds as f64) * 1_000_000_000.0).round() as i64;

    if nanos == 1_000_000_000 {
        seconds += 1;
        nanos = 0;
    }

    if nanos < 0 {
        nanos = 0;
    }

    let nanos_u32 = nanos as u32;

    DateTime::<Utc>::from_timestamp(seconds, nanos_u32)
        .ok_or_else(|| format!("timestamp out of range: {}", raw))
}

#[tool_router]
impl MetricsService {
    #[tool(
        description = "Fetch configured Prometheus metric snapshots and return them in TOON format. Optional `at` parameter accepts RFC3339 timestamps for historical queries."
    )]
    pub async fn get_metric(
        &self,
        params: Parameters<GetMetricRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        let at = request
            .at
            .as_deref()
            .map(|timestamp| {
                DateTime::parse_from_rfc3339(timestamp)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|err| {
                        ErrorData::invalid_params(
                            "Error: at: invalid timestamp",
                            Some(json!({
                                "reason": err.to_string(),
                                "value": timestamp,
                            })),
                        )
                    })
            })
            .transpose()?;

        let mut rows = Vec::with_capacity(self.queries.len());
        for query in &self.queries {
            let row = self.query_metric(query, at).await?;
            rows.push(row);
        }

        let response = Self::build_toon(&rows);

        Ok(CallToolResult::success(vec![Content::text(response)]))
    }
}

#[tool_handler]
impl ServerHandler for MetricsService {
    fn get_info(&self) -> ServerInfo {
        let mut implementation = Implementation::from_build_env();
        if implementation.title.is_none() {
            implementation.title = Some("Metrics MCP Server".into());
        }

        ServerInfo {
            instructions: Some(
                "Metrics MCP server. Use get_metric to read predefined Prometheus/VictoriaMetrics queries. Metrics are configured via METRICS_QUERIES and rendered in TOON format."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: implementation,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_queries_skips_invalid_lines() {
        let input =
            "\n# comment\ntemperature=avg(metric)\ninvalid\nrequests=\n latency=max(latency)\n";
        let queries = MetricsService::parse_queries(input);
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].name, "temperature");
        assert_eq!(queries[1].name, "latency");
    }

    #[test]
    fn parse_timestamp_handles_fractional_seconds() {
        let ts = parse_timestamp("1696452892.25").expect("timestamp parses");
        assert_eq!(ts.timestamp(), 1696452892);
        assert_eq!(ts.timestamp_subsec_nanos(), 250_000_000);
    }
}
