use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::env;
use std::error::Error;

mod service;

use service::WeatherService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let location_path = env::var("LOCATION_PATH")
        .map_err(|_| "Environment variable LOCATION_PATH is required but not found")?;

    let service = WeatherService::new(location_path);

    println!("start server, connect to standard input/output");

    let service = service.serve(stdio()).await?;
    let reason = service.waiting().await?;
    eprintln!("MCP server stopped: {:?}", reason);

    Ok(())
}
