use rmcp::model::CallToolRequestParam;
use rmcp::service::ServiceExt;
use rmcp::transport::TokioChildProcess;
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command as TokioCommand;

fn create_server_command() -> TokioCommand {
    let binary_path = env!("CARGO_BIN_EXE_shell-exec");

    let mut command = TokioCommand::new(binary_path);
    command
        .env("MCP_EXEC_MAX_OUTPUT_BYTES", "4096")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    command
}

fn start_server() -> TokioChildProcess {
    let command = create_server_command();
    TokioChildProcess::new(command).expect("Failed to start MCP server")
}

#[tokio::test]
async fn test_execute_echo() {
    let service = ().serve(start_server()).await.unwrap();

    let response = service
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
}
