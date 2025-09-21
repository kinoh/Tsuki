use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::env;
use std::error::Error;
mod service;
use service::StructuredMemoryService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let data_dir = env::var("DATA_DIR")
        .map_err(|_| "Environment variable DATA_DIR is required but not found")?;

    let root_template = env::var("ROOT_TEMPLATE")
        .map_err(|_| "Environment variable ROOT_TEMPLATE is required but not found")?;

    let service = StructuredMemoryService::new(data_dir, root_template);

    println!("start server, connect to standard input/output");

    let service = service.serve(stdio()).await?;
    let reason = service.waiting().await?;
    eprintln!("MCP server stopped: {:?}", reason);

    Ok(())
}
