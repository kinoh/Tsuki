use rmcp::model::{CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation};
use rmcp::service::ServiceExt;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use serde_json::Value;
use shell_exec::service::ShellExecService;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

async fn start_server() -> (SocketAddr, oneshot::Sender<()>) {
    let service = ShellExecService::from_env().expect("Failed to create service");
    let service_factory = move || Ok(service.clone());
    let service = StreamableHttpService::new(
        service_factory,
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind listener");
    let addr = listener.local_addr().expect("Failed to read listener addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    (addr, shutdown_tx)
}

#[tokio::test]
async fn test_execute_echo() {
    unsafe {
        std::env::set_var("MCP_EXEC_MAX_OUTPUT_BYTES", "4096");
    }
    let (addr, shutdown) = start_server().await;
    let transport =
        StreamableHttpClientTransport::from_uri(format!("http://{addr}/mcp"));
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "shell-exec-test".to_string(),
            version: "0.1.0".to_string(),
        },
    };
    let client = client_info.serve(transport).await.unwrap();

    let response = client
        .call_tool(CallToolRequestParam {
            name: "execute".into(),
            arguments: Some(rmcp::object!({
                "command": "sh",
                "args": ["-c", "echo hello"],
            })),
        })
        .await
        .unwrap();

    assert_eq!(response.is_error, Some(false));
    assert_eq!(response.content.len(), 1);

    let raw = response.content[0].raw.as_text().unwrap().text.clone();
    let value: Value = serde_json::from_str(&raw).expect("Invalid JSON response");

    let stdout = value
        .get("stdout")
        .and_then(|v| v.as_str())
        .expect("stdout missing");
    let exit_code = value
        .get("exit_code")
        .and_then(|v| v.as_i64())
        .expect("exit_code missing");

    assert!(stdout.contains("hello"));
    assert_eq!(exit_code, 0);

    client.cancel().await.unwrap();
    let _ = shutdown.send(());
}
