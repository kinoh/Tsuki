mod service;

use std::{env, error::Error, io, time::Duration};

use chrono_tz::Tz;
use reqwest::Url;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use service::MetricsService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let base_url = env::var("PROMETHEUS_BASE_URL")
        .map_err(|_| "Environment variable PROMETHEUS_BASE_URL is required but not set")?;
    let queries_raw = env::var("METRICS_QUERIES")
        .map_err(|_| "Environment variable METRICS_QUERIES is required but not set")?;
    let tz_name = env::var("TZ").map_err(|_| "Environment variable TZ is required but not set")?;

    let timezone: Tz = tz_name
        .parse()
        .map_err(|_| format!("Invalid TZ value '{}'", tz_name))?;

    let timeout_secs = env::var("HTTP_TIMEOUT_SECONDS")
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|err| format!("Invalid HTTP_TIMEOUT_SECONDS '{}': {}", value, err))
        })
        .transpose()?
        .unwrap_or(10);

    let queries = MetricsService::parse_queries(&queries_raw);
    let base_url = Url::parse(&base_url)
        .map_err(|err| format!("Invalid PROMETHEUS_BASE_URL '{}': {}", base_url, err))?;
    let query_url = MetricsService::build_query_url(&base_url)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;
    let client = MetricsService::build_client(Duration::from_secs(timeout_secs))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;
    let service = MetricsService::new(client, query_url, timezone, queries)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, format!("{:?}", err)))?;

    println!("start server, connect to standard input/output");

    let server = service.serve(stdio()).await?;
    let reason = server.waiting().await?;
    eprintln!("MCP server stopped: {:?}", reason);

    Ok(())
}
