use rmcp::{transport::stdio, ServiceExt};
use shell_exec::service::ShellExecService;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let service = ShellExecService::from_env()?;

    eprintln!("start server, connect to standard input/output");

    let service = service.serve(stdio()).await?;
    let reason = service.waiting().await?;
    eprintln!("MCP server stopped: {:?}", reason);

    Ok(())
}
