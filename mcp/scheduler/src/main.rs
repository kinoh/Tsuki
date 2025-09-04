use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::env;
use std::error::Error;
use std::sync::Arc;
mod service;
use service::SchedulerService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Get data directory from environment variable or use default
    let data_dir = env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    
    let service = Arc::new(SchedulerService::new(data_dir)?);

    eprintln!("Scheduler MCP server starting...");
    eprintln!("TZ environment variable: {}", env::var("TZ").unwrap_or_else(|_| "not set".to_string()));
    eprintln!("Connect to standard input/output");

    // Start the scheduler daemon in the background
    let daemon_service = service.clone();
    tokio::spawn(async move {
        if let Err(e) = daemon_service.start_scheduler_daemon().await {
            eprintln!("Scheduler daemon error: {:?}", e);
        }
    });

    // Start the MCP server
    let mcp_service = (*service).clone().serve(stdio()).await?;
    mcp_service.waiting().await?;

    Ok(())
}
