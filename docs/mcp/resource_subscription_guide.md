# How to Implement Resource Subscription in RMCP

## Resource Subscribe Concept in MCP

The MCP protocol provides resource subscription functionality that enables:
- Clients can monitor changes to specific resources
- Servers send notifications when resources are updated
- Real-time tracking of resource state changes

## Implementation Methods

### 1. Override ServerHandler's subscribe/unsubscribe methods

The key to proper MCP resource notifications is storing `Peer<RoleServer>` instances from the RequestContext, not channels:

```rust
use rmcp::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ResourceServer {
    // Store Peer instances for proper MCP notifications
    subscriptions: Arc<Mutex<HashMap<String, Peer<RoleServer>>>>,
}

impl ServerHandler for ResourceServer {
    async fn subscribe(
        &self,
        request: SubscribeRequestParam,
        context: RequestContext<RoleServer>, // Don't ignore this!
    ) -> Result<(), ErrorData> {
        {
            let mut subscriptions = self.subscriptions.lock().await;
            // Store the peer from context - this is the key!
            subscriptions.insert(request.uri.clone(), context.peer);
        }

        eprintln!("Subscribed to resource: {}", request.uri);
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let mut subscriptions = self.subscriptions.lock().await;
        subscriptions.remove(&request.uri);

        eprintln!("Unsubscribed from resource: {}", request.uri);
        Ok(())
    }
}
```

### 2. Send notifications using peer.notify_resource_updated()

When resources change, use the stored peer to send notifications:

```rust
impl ResourceServer {
    pub async fn update_resource(&self, uri: &str, title: &str) {
        // Resource update processing...

        // Send notification to subscribers using stored peers
        let subscriptions = self.subscriptions.lock().await;
        if let Some(peer) = subscriptions.get(uri) {
            let params = ResourceUpdatedNotificationParam {
                uri: uri.to_string(),
                title: title.to_string(), // MCP 2025-06-18 feature
            };

            if let Err(e) = peer.notify_resource_updated(params).await {
                eprintln!("Failed to send resource update notification: {:?}", e);
            }
        }
    }
}
```

### 3. Enable subscribe functionality in ServerCapabilities

```rust
impl ServerHandler for ResourceServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder()
                .enable_resources()
                .enable_resources_subscribe()  // Correct method name
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Resource server with subscribe support".to_string()),
        }
    }
}
```

## Complete Working Example

Here's a complete, working implementation based on the scheduler MCP server:

```rust
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{
        ServerCapabilities, ServerInfo, SubscribeRequestParam, UnsubscribeRequestParam,
        ResourceUpdatedNotificationParam, Implementation,
    },
    service::RequestContext,
};
use rmcp::{Peer, RoleServer};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct SchedulerService {
    subscriptions: Arc<Mutex<HashMap<String, Peer<RoleServer>>>>,
    // ... other fields
}

impl SchedulerService {
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            // ... initialize other fields
        }
    }

    // This method is called when a schedule fires
    pub async fn notify_fired_schedule(&self, message: &str) {
        let subscriptions = self.subscriptions.lock().await;

        for (uri, peer) in subscriptions.iter() {
            if uri.starts_with("fired_schedule://") {
                let params = ResourceUpdatedNotificationParam {
                    uri: uri.clone(),
                    title: message.to_string(), // Schedule message as title
                };

                if let Err(e) = peer.notify_resource_updated(params).await {
                    eprintln!("Failed to send resource update notification: {:?}", e);
                }
            }
        }
    }
}

impl ServerHandler for SchedulerService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: rmcp::model::ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder()
                .enable_resources()
                .enable_resources_subscribe()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Scheduler MCP server with subscription support".to_string()),
        }
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParam,
        context: RequestContext<RoleServer>, // Use the context!
    ) -> Result<(), ErrorData> {
        {
            let mut subscriptions = self.subscriptions.lock().await;
            subscriptions.insert(request.uri.clone(), context.peer);
        }

        eprintln!("Subscribed to resource: {}", request.uri);
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let mut subscriptions = self.subscriptions.lock().await;
        subscriptions.remove(&request.uri);

        eprintln!("Unsubscribed from resource: {}", request.uri);
        Ok(())
    }

    // ... implement other required methods (list_resources, read_resource, etc.)
}
```

## Key Points

1. **Store Peer instances, not channels**: The `context.peer` from `subscribe()` is what enables proper MCP notifications.

2. **Use peer.notify_resource_updated()**: This is the correct way to send resource update notifications to subscribed clients.

3. **Include title field**: MCP protocol version 2025-06-18 adds a `title` field to resource notifications for better context.

4. **Enable correct capability**: Use `enable_resources_subscribe()` method in ServerCapabilities.

This implementation ensures proper MCP protocol compliance and enables real-time resource notifications to subscribed clients.