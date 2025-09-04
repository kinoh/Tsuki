use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::env;
use std::error::Error;
mod service;
use service::SchedulerService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Get data directory from environment variable or use default
    let data_dir = env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    
    let service = SchedulerService::new(data_dir)?;

    eprintln!("Scheduler MCP server starting...");
    eprintln!("TZ environment variable: {}", env::var("TZ").unwrap_or_else(|_| "not set".to_string()));
    eprintln!("Connect to standard input/output");

    let service = service.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
