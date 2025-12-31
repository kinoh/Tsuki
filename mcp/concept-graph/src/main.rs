use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::env;
use std::error::Error;

mod service;
use service::ConceptGraphService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let uri = env::var("MEMGRAPH_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string());
    let user = env::var("MEMGRAPH_USER").unwrap_or_default();
    let password = env::var("MEMGRAPH_PASSWORD").unwrap_or_default();
    let arousal_tau_ms = env::var("AROUSAL_TAU_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(86_400_000.0);

    let service = ConceptGraphService::connect(uri, user, password, arousal_tau_ms).await?;

    println!("start server, connect to standard input/output");

    let service = service.serve(stdio()).await?;
    let reason = service.waiting().await?;
    eprintln!("MCP server stopped: {:?}", reason);

    Ok(())
}
