# How to Implement Resource Subscription in RMCP

## Resource Subscribe Concept in MCP

The MCP protocol provides resource subscription functionality that enables:
- Clients can monitor changes to specific resources
- Servers send notifications when resources are updated
- Real-time tracking of resource state changes

## Implementation Methods

1. Override ServerHandler's subscribe/unsubscribe methods

```rust
use rmcp::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

#[derive(Clone)]
pub struct ResourceServer {
    // subscribed URIs to notification sender
    subscriptions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<ResourceUpdatedNotification>>>>,
    notification_sender: mpsc::UnboundedSender<ResourceUpdatedNotification>,
}

impl ServerHandler for ResourceServer {
    // Other method implementations...

    async fn subscribe(
        &self,
        request: SubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        let mut subscriptions = self.subscriptions.lock().await;

        // Add to subscription list
        subscriptions.insert(request.uri.clone(), self.notification_sender.clone());

        tracing::info!("Subscribed to resource: {}", request.uri);
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        let mut subscriptions = self.subscriptions.lock().await;

        // Remove from subscription list
        subscriptions.remove(&request.uri);

        tracing::info!("Unsubscribed from resource: {}", request.uri);
        Ok(())
    }
}
```

2. Send notifications when resources change

```rust
impl ResourceServer {
    pub async fn update_resource(&self, uri: &str, new_content: &str) {
        // Resource update processing...

        // Send notification to subscribers
        let subscriptions = self.subscriptions.lock().await;
        if let Some(sender) = subscriptions.get(uri) {
            let notification = ResourceUpdatedNotification {
                method: ResourceUpdatedNotificationMethod,
                params: ResourceUpdatedNotificationParam {
                    uri: uri.to_string(),
                },
            };

            if let Err(e) = sender.send(notification) {
                tracing::error!("Failed to send resource update notification: {}", e);
            }
        }
    }
}
```

3. Enable subscribe functionality in ServerCapabilities

```rust
impl ServerHandler for ResourceServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_resources()
                .with_resource_subscribe(true)  // Enable subscribe
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Resource server with subscribe support".to_string()),
        }
    }
}
```

4. Complete implementation example

```rust
use rmcp::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{interval, Duration};

#[derive(Clone)]
pub struct SubscribableResourceServer {
    resources: Arc<Mutex<HashMap<String, String>>>,
    subscriptions: Arc<Mutex<HashMap<String, Vec<mpsc::UnboundedSender<String>>>>>,
}

impl SubscribableResourceServer {
    pub fn new() -> Self {
        Self {
            resources: Arc::new(Mutex::new(HashMap::new())),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // Update resource and notify subscribers
    pub async fn update_resource(&self, uri: &str, content: String) {
        {
            let mut resources = self.resources.lock().await;
            resources.insert(uri.to_string(), content.clone());
        }

        // Notify subscribers
        let subscriptions = self.subscriptions.lock().await;
        if let Some(senders) = subscriptions.get(uri) {
            for sender in senders {
                // Actual notification is handled at service layer
                let _ = sender.send(uri.to_string());
            }
        }
    }

    // Example task for periodic resource updates
    pub async fn start_periodic_updates(&self) {
        let server = self.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5));
            let mut counter = 0;

            loop {
                interval.tick().await;
                counter += 1;

                let content = format!("Updated content at {}",
                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"));
                server.update_resource("resource://counter", content).await;
            }
        });
    }
}

impl ServerHandler for SubscribableResourceServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_resources()
                .with_resource_subscribe(true)
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Subscribable resource server example".to_string()),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                RawResource::new("resource://counter", "counter").no_annotation(),
                RawResource::new("resource://status", "status").no_annotation(),
            ],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        ReadResourceRequestParam { uri }: ReadResourceRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let resources = self.resources.lock().await;

        if let Some(content) = resources.get(&uri) {
            Ok(ReadResourceResult {
                contents: vec![ResourceContents::text(content, &uri)],
            })
        } else {
            Err(McpError::resource_not_found(
                "Resource not found",
                Some(serde_json::json!({"uri": uri})),
            ))
        }
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        let (sender, mut receiver) = mpsc::unbounded_channel();

        {
            let mut subscriptions = self.subscriptions.lock().await;
            subscriptions.entry(request.uri.clone())
                .or_insert_with(Vec::new)
                .push(sender);
        }

        // Run notification receiving process in background
        let uri = request.uri.clone();
        tokio::spawn(async move {
            while let Some(_update) = receiver.recv().await {
                // Send actual notification here
                // Typically use sending functionality from service or handler context
                tracing::info!("Resource updated: {}", uri);
            }
        });

        tracing::info!("Subscribed to resource: {}", request.uri);
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        let mut subscriptions = self.subscriptions.lock().await;
        subscriptions.remove(&request.uri);

        tracing::info!("Unsubscribed from resource: {}", request.uri);
        Ok(())
    }
}
```

5. Server startup

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let server = SubscribableResourceServer::new();

    // Start periodic updates
    server.start_periodic_updates().await;

    // Start server
    server.serve(stdio()).await?.waiting().await?;
    Ok(())
}
```

This implementation enables:
- Clients can subscribe to resources using resources/subscribe
- Server sends notifications/resources/updated when resources are updated
- Resource unsubscription is possible with resources/unsubscribe