use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use shell_exec::service::ShellExecService;
use std::{env, error::Error, io};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let bind = env::var("MCP_HTTP_BIND").unwrap_or_else(|_| "0.0.0.0:8000".to_string());
    let mut path = env::var("MCP_HTTP_PATH").unwrap_or_else(|_| "/mcp".to_string());
    if !path.starts_with('/') {
        path = format!("/{}", path);
    }

    let service = ShellExecService::from_env().map_err(|err| {
        io::Error::new(io::ErrorKind::Other, err.message.to_string())
    })?;
    let service_factory = move || Ok(service.clone());
    let service = StreamableHttpService::new(
        service_factory,
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    let router = axum::Router::new().nest_service(&path, service);
    let listener = tokio::net::TcpListener::bind(&bind).await?;

    eprintln!("start server, bind={} path={}", bind, path);

    axum::serve(listener, router).await?;

    Ok(())
}
