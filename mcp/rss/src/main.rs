use rmcp::{transport::stdio, ServiceExt};
use rss_mcp::service::RssService;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let service = RssService::from_env().await?;

    println!("start server, connect to standard input/output");

    let service = service.serve(stdio()).await?;
    let reason = service.waiting().await?;
    eprintln!("MCP server stopped: {:?}", reason);

    Ok(())
}
