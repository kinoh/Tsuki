use rmcp::model::{
    ReadResourceRequestParam, ResourceContents, ResourceUpdatedNotificationParam,
    ServerCapabilities, SubscribeRequestParam, ToolsCapability,
};
use rmcp::transport::TokioChildProcess;
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{ClientHandler, ServiceError};
use rmcp::{model::CallToolRequestParam, service::ServiceExt};
use std::process::Stdio;
use tempfile::TempDir;
use tokio::process::Command as TokioCommand;
use tokio::sync::mpsc;

/// Helper function to create temporary directory for tests
fn setup_test_env() -> TempDir {
    TempDir::new().expect("Failed to create temporary directory")
}

fn create_server_command(temp_dir: &TempDir) -> TokioCommand {
    let binary_path = env!("CARGO_BIN_EXE_scheduler");

    let mut command = TokioCommand::new(binary_path);
    command
        .env("SCHEDULER_LOOP_INTERVAL_MS", "100")
        .env("TZ", "UTC")
        .env("DATA_DIR", temp_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()); // Show stderr for debugging

    command
}

fn start_server(temp_dir: &TempDir) -> TokioChildProcess {
    let command = create_server_command(temp_dir);
    TokioChildProcess::new(command).unwrap()
}

#[tokio::test]
async fn test_server_initialization() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    let response = service.peer_info().unwrap();

    assert_eq!(
        response.capabilities,
        ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: None }),
            resources: Some(rmcp::model::ResourcesCapability {
                subscribe: Some(true),
                list_changed: None,
            }),
            ..Default::default()
        }
    );
    assert!(response.instructions.is_some());
}

#[tokio::test]
async fn test_list_tools() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    let response = service.list_tools(None).await.unwrap();

    let tool_names: Vec<&str> = response.tools.iter().map(|t| t.name.as_ref()).collect();

    assert!(tool_names.contains(&"set_schedule"));
    assert!(tool_names.contains(&"remove_schedule"));
}

#[tokio::test]
async fn test_list_resources() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    let response = service.list_resources(None).await.unwrap();

    // Check that fired_schedule resource is available
    assert_eq!(response.resources.len(), 1);
    let resource = &response.resources[0];
    assert_eq!(resource.uri.as_str(), "fired_schedule://recent");
    assert_eq!(resource.name.as_str(), "Recent Fired Schedules");
}

#[tokio::test]
async fn test_set_schedule_daily() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    // Set a daily schedule
    let response = service
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "test_daily",
                "time": "09:00",
                "cycle": "daily",
                "message": "Daily reminder"
            })),
        })
        .await
        .unwrap();

    assert_eq!(response.is_error, Some(false));
    assert_eq!(response.content.len(), 1);
    assert_eq!(response.content[0].raw.as_text().unwrap().text, "Succeeded");
}

#[tokio::test]
async fn test_set_schedule_one_time() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    // Set a once schedule
    let response = service
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "test_onetime",
                "time": "2024-12-31T23:59:59+00:00",
                "cycle": "once",
                "message": "New Year reminder"
            })),
        })
        .await
        .unwrap();

    assert_eq!(response.is_error, Some(false));
    assert_eq!(response.content.len(), 1);
    assert_eq!(response.content[0].raw.as_text().unwrap().text, "Succeeded");
}

#[tokio::test]
async fn test_set_schedule_validation_errors() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    // Test empty name
    let response = service
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "",
                "time": "09:00",
                "cycle": "daily",
                "message": "Test message"
            })),
        })
        .await;

    let error = match response {
        Err(ServiceError::McpError(data)) => data,
        _ => panic!("Expected McpError"),
    };
    assert_eq!(error.message, "Error: name: required");

    // Test empty message
    let response = service
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "test",
                "time": "09:00",
                "cycle": "daily",
                "message": ""
            })),
        })
        .await;

    let error = match response {
        Err(ServiceError::McpError(data)) => data,
        _ => panic!("Expected McpError"),
    };
    assert_eq!(error.message, "Error: message: required");

    // Test invalid cycle
    let response = service
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "test",
                "time": "09:00",
                "cycle": "invalid",
                "message": "Test message"
            })),
        })
        .await;

    let error = match response {
        Err(ServiceError::McpError(data)) => data,
        _ => panic!("Expected McpError"),
    };
    assert_eq!(error.message, "Error: cycle: invalid value");
}

#[tokio::test]
async fn test_remove_schedule() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    // First, set a schedule
    let set_response = service
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "test_remove",
                "time": "10:00",
                "cycle": "daily",
                "message": "To be removed"
            })),
        })
        .await
        .unwrap();
    assert_eq!(set_response.is_error, Some(false));

    // Then remove it
    let response = service
        .call_tool(CallToolRequestParam {
            name: "remove_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "test_remove"
            })),
        })
        .await
        .unwrap();

    assert_eq!(response.is_error, Some(false));
    assert_eq!(response.content.len(), 1);
    assert_eq!(response.content[0].raw.as_text().unwrap().text, "Succeeded");
}

#[tokio::test]
async fn test_remove_nonexistent_schedule() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    // Try to remove a schedule that doesn't exist
    let response = service
        .call_tool(CallToolRequestParam {
            name: "remove_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "nonexistent"
            })),
        })
        .await;

    let error = match response {
        Err(ServiceError::McpError(data)) => data,
        _ => panic!("Expected McpError"),
    };
    assert_eq!(error.message, "Error: name: not found");
}

#[tokio::test]
async fn test_data_persistence() {
    let temp_dir = setup_test_env();

    // First session: create schedule
    {
        let service = ().serve(start_server(&temp_dir)).await.unwrap();
        let set_response = service
            .call_tool(CallToolRequestParam {
                name: "set_schedule".into(),
                arguments: Some(rmcp::object!({
                    "name": "persistent_test",
                    "time": "11:00",
                    "cycle": "daily",
                    "message": "Persistent schedule"
                })),
            })
            .await
            .unwrap();
        assert_eq!(set_response.is_error, Some(false));
    }

    // Second session: verify schedule persists
    {
        let service = ().serve(start_server(&temp_dir)).await.unwrap();
        let response = service
            .call_tool(CallToolRequestParam {
                name: "remove_schedule".into(),
                arguments: Some(rmcp::object!({
                    "name": "persistent_test"
                })),
            })
            .await
            .unwrap();
        assert_eq!(response.is_error, Some(false));
        assert_eq!(response.content.len(), 1);
        assert_eq!(response.content[0].raw.as_text().unwrap().text, "Succeeded");
    }
}

#[tokio::test]
async fn test_read_fired_schedules_resource() {
    let temp_dir = setup_test_env();
    let service = ().serve(start_server(&temp_dir)).await.unwrap();

    // Read the fired schedules resource
    let response = service
        .read_resource(ReadResourceRequestParam {
            uri: "fired_schedule://recent".into(),
        })
        .await
        .unwrap();

    assert_eq!(response.contents.len(), 1);
    assert_eq!(
        response.contents[0],
        ResourceContents::TextResourceContents {
            uri: "fired_schedule://recent".to_string(),
            mime_type: Some("application/json".to_string()),
            text: "[]".to_string(),
            meta: None
        }
    );
}

pub struct Client {
    notification_channel: mpsc::Sender<ResourceUpdatedNotificationParam>,
}

impl ClientHandler for Client {
    async fn on_resource_updated(
        &self,
        params: rmcp::model::ResourceUpdatedNotificationParam,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        let _ = self
            .notification_channel
            .send(params.clone())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn test_resource_subscription() {
    let temp_dir = setup_test_env();

    // Start the server process
    let mut child = create_server_command(&temp_dir).spawn().unwrap();

    // Client transport using the child's stdin and stdout
    let stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let client_transport = AsyncRwTransport::new(stdout, stdin);

    // Notification channel
    let (tx, mut rx) = mpsc::channel(8);

    // Create the client
    let client = Client {
        notification_channel: tx,
    }
    .serve(client_transport)
    .await
    .unwrap();

    // Subscribe to the fired_schedule resource
    client
        .subscribe(SubscribeRequestParam {
            uri: "fired_schedule://recent".into(),
        })
        .await
        .unwrap();

    // Set a schedule that will fire soon (once)
    let time = chrono::Local::now();
    let response = client
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "sub_test",
                "time": time.to_rfc3339(),
                "cycle": "once",
                "message": "Subscription test"
            })),
        })
        .await
        .unwrap();
    assert_eq!(response.is_error, Some(false));

    // Wait for the notification
    let notification = rx.recv().await.unwrap();
    assert_eq!(
        notification,
        rmcp::model::ResourceUpdatedNotificationParam {
            uri: "fired_schedule://recent".to_string(),
            title: "Subscription test".to_string()
        }
    );

    // No more notifications should be received
    let no_notification = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;
    assert!(no_notification.is_err(), "Unexpected notification received");

    client.cancel().await.unwrap();
}

#[tokio::test]
async fn test_resource_subscription_daily() {
    let temp_dir = setup_test_env();

    // Start the server process
    let mut child = create_server_command(&temp_dir).spawn().unwrap();

    // Client transport using the child's stdin and stdout
    let stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let client_transport = AsyncRwTransport::new(stdout, stdin);

    // Notification channel
    let (tx, mut rx) = mpsc::channel(8);

    // Create the client
    let client = Client {
        notification_channel: tx,
    }
    .serve(client_transport)
    .await
    .unwrap();

    // Subscribe to the fired_schedule resource
    client
        .subscribe(SubscribeRequestParam {
            uri: "fired_schedule://recent".into(),
        })
        .await
        .unwrap();

    // Set a daily schedule that should fire immediately
    // Use UTC to match server's timezone (TZ=UTC)
    let now = chrono::Utc::now();
    let time_str = now.format("%H:%M:%S").to_string();
    let response = client
        .call_tool(CallToolRequestParam {
            name: "set_schedule".into(),
            arguments: Some(rmcp::object!({
                "name": "sub_test_daily",
                "time": time_str,
                "cycle": "daily",
                "message": "Subscription test daily"
            })),
        })
        .await
        .unwrap();
    assert_eq!(response.is_error, Some(false));

    // Wait for the notification (should fire within a short interval)
    let notification = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
        .await
        .expect("No notification received")
        .unwrap();
    assert_eq!(
        notification,
        rmcp::model::ResourceUpdatedNotificationParam {
            uri: "fired_schedule://recent".to_string(),
            title: "Subscription test daily".to_string()
        }
    );

    // No more notifications should be received immediately after
    let no_notification = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await;
    assert!(no_notification.is_err(), "Unexpected notification received");

    client.cancel().await.unwrap();
}
