use rmcp::serve_server;
use std::env;
use std::error::Error;
mod service;
use service::StructuredMemoryService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let data_dir = env::var("DATA_DIR")
        .map_err(|_| "Environment variable DATA_DIR is required but not found")?;

    let service = StructuredMemoryService::new(data_dir);

    println!("start server, connect to standard input/output");

    let io = (tokio::io::stdin(), tokio::io::stdout());

    serve_server(service, io).await?;
    Ok(())
}
