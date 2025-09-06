use chrono::Local;
use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child as TokioChild, ChildStdin, ChildStdout, Command as TokioCommand};
use tokio::time::timeout;

/// MCP client for testing scheduler server
pub struct McpClient {
    child: TokioChild,
    stdin: ChildStdin,
    stdout_lines: Lines<BufReader<ChildStdout>>,
    request_id: u64,
}

impl McpClient {
    /// Start scheduler server and create MCP client
    pub async fn new(temp_dir: &TempDir) -> Result<Self, Box<dyn std::error::Error>> {
        let binary_path = env!("CARGO_BIN_EXE_scheduler");

        let mut child = TokioCommand::new(binary_path)
            .env("SCHEDULER_LOOP_INTERVAL_MS", "100")
            .env("TZ", "UTC")
            .env("DATA_DIR", temp_dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // Show stderr for debugging
            .spawn()?;

        let stdin = child.stdin.take().expect("Failed to get stdin");
        let stdout = child.stdout.take().expect("Failed to get stdout");
        let stdout_reader = BufReader::new(stdout);
        let stdout_lines = stdout_reader.lines();

        // Wait for server startup message
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(Self {
            child,
            stdin,
            stdout_lines,
            request_id: 1,
        })
    }

    /// Send initialization request to MCP server
    pub async fn initialize(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "initialize",
            "params": {
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                },
                "protocolVersion": "2024-11-05"
            }
        });

        let response = self.send_request(request).await?;

        // Send initialized notification
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });

        let notification_str = serde_json::to_string(&notification)?;
        self.stdin.write_all(notification_str.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        // Wait a moment for the server to process the notification
        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(response)
    }

    /// Send call_tool request
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        });

        self.send_request(request).await
    }

    /// Send list_tools request
    pub async fn list_tools(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "tools/list",
            "params": {}
        });

        self.send_request(request).await
    }

    /// Send list_resources request
    pub async fn list_resources(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "resources/list",
            "params": {}
        });

        self.send_request(request).await
    }

    /// Send read_resource request
    pub async fn read_resource(&mut self, uri: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "resources/read",
            "params": {
                "uri": uri
            }
        });

        self.send_request(request).await
    }

    /// Subscribe to a resource
    pub async fn subscribe_resource(
        &mut self,
        uri: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "resources/subscribe",
            "params": { "uri": uri }
        });
        self.send_request(request).await?; // ignore response content
        Ok(())
    }

    /// Wait for a resource/updated notification (returns the notification JSON)
    pub async fn wait_for_resource_update(
        &mut self,
        timeout_sec: u64,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let notification = timeout(Duration::from_secs(timeout_sec), async {
            loop {
                match self.stdout_lines.next_line().await {
                    Ok(Some(line)) => {
                        if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                            if msg.get("method") == Some(&json!("notifications/resources/updated"))
                            {
                                return Ok::<Value, Box<dyn std::error::Error>>(msg);
                            } else {
                                return Err(format!("Unexpected message: {:?}", msg).into());
                            }
                        }
                    }
                    Ok(None) => return Err("Stream ended".into()),
                    Err(e) => return Err(format!("IO error: {}", e).into()),
                }
            }
        })
        .await??;
        Ok(notification)
    }

    /// Send JSON-RPC request and wait for response
    async fn send_request(&mut self, request: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let request_str = serde_json::to_string(&request)?;
        self.stdin.write_all(request_str.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        let current_id = self.request_id;
        self.request_id += 1;

        // Wait for response with timeout
        let response = match timeout(Duration::from_secs(5), async {
            loop {
                match self.stdout_lines.next_line().await {
                    Ok(Some(line)) => {
                        if let Ok(response) = serde_json::from_str::<Value>(&line) {
                            if let Some(id) = response.get("id") {
                                if id.as_u64() == Some(current_id) {
                                    return response;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        panic!("Stream ended");
                    }
                    Err(e) => {
                        panic!("IO error: {}", e);
                    }
                }
            }
        })
        .await
        {
            Ok(response) => response,
            Err(_) => return Err("Timeout waiting for response".into()),
        };

        Ok(response)
    }

    /// Terminate the server process
    pub async fn shutdown(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        Ok(())
    }
}

/// Helper function to create temporary directory for tests
fn setup_test_env() -> TempDir {
    TempDir::new().expect("Failed to create temporary directory")
}

#[tokio::test]
async fn test_server_initialization() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    let response = client.initialize().await.unwrap();

    assert!(response.get("result").is_some());
    assert_eq!(response.get("jsonrpc").unwrap(), "2.0");

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_list_tools() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();
    let response = client.list_tools().await.unwrap();

    let result = response.get("result").unwrap();
    let tools = result.get("tools").unwrap().as_array().unwrap();

    // Check that set_schedule and remove_schedule tools are available
    let tool_names: Vec<&str> = tools
        .iter()
        .map(|tool| tool.get("name").unwrap().as_str().unwrap())
        .collect();

    assert!(tool_names.contains(&"set_schedule"));
    assert!(tool_names.contains(&"remove_schedule"));

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_list_resources() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();
    let response = client.list_resources().await.unwrap();

    let result = response.get("result").unwrap();
    let resources = result.get("resources").unwrap().as_array().unwrap();

    // Check that fired_schedule resource is available
    assert_eq!(resources.len(), 1);
    let resource = &resources[0];
    assert_eq!(
        resource.get("uri").unwrap().as_str().unwrap(),
        "fired_schedule://recent"
    );
    assert_eq!(
        resource.get("name").unwrap().as_str().unwrap(),
        "Recent Fired Schedules"
    );

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_set_schedule_daily() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();

    // Set a daily schedule
    let response = client
        .call_tool(
            "set_schedule",
            json!({
                "name": "test_daily",
                "time": "09:00",
                "cycle": "daily",
                "message": "Daily reminder"
            }),
        )
        .await
        .unwrap();

    let result = response.get("result").unwrap();
    assert_eq!(result.get("isError"), Some(&json!(false)));

    let content = result.get("content").unwrap().as_array().unwrap();
    assert_eq!(content[0].get("text").unwrap(), "Succeeded");

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_set_schedule_one_time() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();

    // Set a one-time schedule
    let response = client
        .call_tool(
            "set_schedule",
            json!({
                "name": "test_onetime",
                "time": "2024-12-31T23:59:59+00:00",
                "cycle": "none",
                "message": "New Year reminder"
            }),
        )
        .await
        .unwrap();

    let result = response.get("result").unwrap();
    assert_eq!(result.get("isError"), Some(&json!(false)));

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_set_schedule_validation_errors() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();

    // Test empty name
    let response = client
        .call_tool(
            "set_schedule",
            json!({
                "name": "",
                "time": "09:00",
                "cycle": "daily",
                "message": "Test message"
            }),
        )
        .await
        .unwrap();

    assert!(response.get("error").is_some());

    // Test empty message
    let response = client
        .call_tool(
            "set_schedule",
            json!({
                "name": "test",
                "time": "09:00",
                "cycle": "daily",
                "message": ""
            }),
        )
        .await
        .unwrap();

    assert!(response.get("error").is_some());

    // Test invalid cycle
    let response = client
        .call_tool(
            "set_schedule",
            json!({
                "name": "test",
                "time": "09:00",
                "cycle": "invalid",
                "message": "Test message"
            }),
        )
        .await
        .unwrap();

    assert!(response.get("error").is_some());

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_remove_schedule() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();

    // First, set a schedule
    client
        .call_tool(
            "set_schedule",
            json!({
                "name": "test_remove",
                "time": "10:00",
                "cycle": "daily",
                "message": "To be removed"
            }),
        )
        .await
        .unwrap();

    // Then remove it
    let response = client
        .call_tool(
            "remove_schedule",
            json!({
                "name": "test_remove"
            }),
        )
        .await
        .unwrap();

    let result = response.get("result").unwrap();
    assert_eq!(result.get("isError"), Some(&json!(false)));

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_remove_nonexistent_schedule() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();

    // Try to remove a schedule that doesn't exist
    let response = client
        .call_tool(
            "remove_schedule",
            json!({
                "name": "nonexistent"
            }),
        )
        .await
        .unwrap();

    assert!(response.get("error").is_some());

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_data_persistence() {
    let temp_dir = setup_test_env();

    // First session: create schedule
    {
        let mut client = McpClient::new(&temp_dir).await.unwrap();
        client.initialize().await.unwrap();

        client
            .call_tool(
                "set_schedule",
                json!({
                    "name": "persistent_test",
                    "time": "11:00",
                    "cycle": "daily",
                    "message": "Persistent schedule"
                }),
            )
            .await
            .unwrap();

        client.shutdown().await.unwrap();
    }

    // Second session: verify schedule persists
    {
        let mut client = McpClient::new(&temp_dir).await.unwrap();
        client.initialize().await.unwrap();

        // Try to remove the schedule (this will fail if it doesn't exist)
        let response = client
            .call_tool(
                "remove_schedule",
                json!({
                    "name": "persistent_test"
                }),
            )
            .await
            .unwrap();

        let result = response.get("result").unwrap();
        assert_eq!(result.get("isError"), Some(&json!(false)));

        client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_read_fired_schedules_resource() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();

    client.initialize().await.unwrap();

    // Read the fired schedules resource
    let response = client
        .read_resource("fired_schedule://recent")
        .await
        .unwrap();

    let result = response.get("result").unwrap();
    let contents = result.get("contents").unwrap().as_array().unwrap();

    assert_eq!(contents.len(), 1);
    let content = &contents[0];
    // Check mimeType (might be "text" or "application/json")
    let mime_type = content.get("mimeType").unwrap().as_str().unwrap();
    assert!(mime_type == "application/json" || mime_type == "text");

    // Parse the JSON content
    let text = content.get("text").unwrap().as_str().unwrap();
    let fired_schedules: Value = serde_json::from_str(text).unwrap();
    assert!(fired_schedules.is_array());

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_resource_subscription() {
    let temp_dir = setup_test_env();
    let mut client = McpClient::new(&temp_dir).await.unwrap();
    client.initialize().await.unwrap();

    // Subscribe to the fired_schedule resource
    client
        .subscribe_resource("fired_schedule://recent")
        .await
        .unwrap();

    // Add a schedule to trigger a resource update
    let time = Local::now();

    let response = client
        .call_tool(
            "set_schedule",
            json!({
                "name": "sub_test",
                "time": time.to_rfc3339(),
                "cycle": "none",
                "message": "Subscription test"
            }),
        )
        .await
        .unwrap();
    assert_eq!(response.get("error"), None);

    // Wait for notification
    let notification = client.wait_for_resource_update(1).await.unwrap();
    let params = notification.get("params").unwrap();
    assert_eq!(params.get("uri").unwrap(), "fired_schedule://recent");
    assert_eq!(params.get("title").unwrap(), "Subscription test");

    client.shutdown().await.unwrap();
}
