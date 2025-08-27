use std::error::Error;
use rmcp::serve_server;
mod service;
use service::StructuredMemoryService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let service = StructuredMemoryService::new();

    println!("start server, connect to standard input/output");

    let io = (tokio::io::stdin(), tokio::io::stdout());

    serve_server(service, io).await?;
    Ok(())
}
