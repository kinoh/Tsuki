use async_openai::types::responses::{FunctionTool, Tool};
use rmcp::model::{CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation};
use rmcp::service::ServiceExt;
use rmcp::transport::StreamableHttpClientTransport;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::activation_concept_graph::ConceptGraphStore;
use crate::config::McpServerConfig;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct McpToolDescriptor {
    pub(crate) runtime_tool_name: String,
    pub(crate) server_id: String,
    pub(crate) remote_tool_name: String,
    pub(crate) concept_key: String,
    pub(crate) description: Option<String>,
    pub(crate) parameters: Value,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct McpToolVisibility {
    pub(crate) runtime_tool_name: String,
    pub(crate) concept_key: String,
    pub(crate) score: f64,
    pub(crate) visible: bool,
    pub(crate) reason: String,
}

#[derive(Debug, Clone)]
pub(crate) struct McpRegistry {
    servers: BTreeMap<String, String>,
    tools_by_runtime: BTreeMap<String, McpToolDescriptor>,
}

#[derive(Debug, Clone)]
pub(crate) struct McpBootstrapResult {
    pub(crate) registry: McpRegistry,
    pub(crate) auto_created: Vec<McpAutoCreatedLog>,
    pub(crate) errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct McpAutoCreatedLog {
    pub(crate) server_id: String,
    pub(crate) tool_name: String,
    pub(crate) concept_key: String,
    pub(crate) reason: &'static str,
    pub(crate) result: &'static str,
    pub(crate) phase: &'static str,
}

#[derive(Debug, Deserialize)]
struct ToolObject {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_schema: Option<Value>,
}

impl McpRegistry {
    pub(crate) fn empty() -> Self {
        Self {
            servers: BTreeMap::new(),
            tools_by_runtime: BTreeMap::new(),
        }
    }

    pub(crate) async fn bootstrap(
        servers: &BTreeMap<String, McpServerConfig>,
        concept_graph: &dyn ConceptGraphStore,
    ) -> McpBootstrapResult {
        let mut out = Self::empty();
        let mut auto_created = Vec::<McpAutoCreatedLog>::new();
        let mut errors = Vec::<String>::new();
        let mut concept_keys = BTreeSet::<String>::new();

        for (server_id, cfg) in servers {
            out.servers.insert(server_id.clone(), cfg.url.clone());
            let discovered = match discover_tools(cfg.url.as_str()).await {
                Ok(tools) => tools,
                Err(err) => {
                    errors.push(format!(
                        "mcp bootstrap failed server={} url={} error={}",
                        server_id, cfg.url, err
                    ));
                    continue;
                }
            };

            for discovered_tool in discovered {
                let mapping = match map_tool(server_id, cfg.url.as_str(), &discovered_tool) {
                    Ok(mapping) => mapping,
                    Err(err) => {
                        errors.push(err);
                        continue;
                    }
                };

                if let Some(existing) = out.tools_by_runtime.get(mapping.runtime_tool_name.as_str()) {
                    errors.push(format!(
                        "mcp mapping failed: runtime tool collision runtime_tool={} existing_server={} existing_remote={} new_server={} new_remote={}",
                        mapping.runtime_tool_name,
                        existing.server_id,
                        existing.remote_tool_name,
                        mapping.server_id,
                        mapping.remote_tool_name,
                    ));
                    continue;
                }
                if !concept_keys.insert(mapping.concept_key.clone()) {
                    errors.push(format!(
                        "mcp mapping failed: concept key collision concept_key={} server={} remote_tool={}",
                        mapping.concept_key, mapping.server_id, mapping.remote_tool_name
                    ));
                    continue;
                }

                let concept_upsert = concept_graph
                    .concept_upsert(mapping.concept_key.clone())
                    .await
                    .map(|_| "created")
                    .unwrap_or("already_exists");
                auto_created.push(McpAutoCreatedLog {
                    server_id: mapping.server_id.clone(),
                    tool_name: mapping.remote_tool_name.clone(),
                    concept_key: mapping.concept_key.clone(),
                    reason: "missing_concept",
                    result: concept_upsert,
                    phase: "bootstrap",
                });

                out.tools_by_runtime
                    .insert(mapping.runtime_tool_name.clone(), mapping);
            }
        }

        McpBootstrapResult {
            registry: out,
            auto_created,
            errors,
        }
    }

    pub(crate) fn available_tool_names(&self) -> Vec<String> {
        self.tools_by_runtime.keys().cloned().collect()
    }

    pub(crate) fn available_tools(&self) -> Vec<Tool> {
        self.tools_by_runtime
            .values()
            .map(|item| {
                Tool::Function(FunctionTool {
                    name: item.runtime_tool_name.clone(),
                    description: item.description.clone(),
                    parameters: Some(item.parameters.clone()),
                    strict: Some(false),
                })
            })
            .collect()
    }

    pub(crate) fn contains_runtime_tool(&self, runtime_tool_name: &str) -> bool {
        self.tools_by_runtime.contains_key(runtime_tool_name)
    }

    pub(crate) async fn call_tool(
        &self,
        runtime_tool_name: &str,
        arguments: Value,
    ) -> Result<String, String> {
        let item = self
            .tools_by_runtime
            .get(runtime_tool_name)
            .ok_or_else(|| format!("unknown MCP runtime tool: {}", runtime_tool_name))?;
        let endpoint = self
            .servers
            .get(item.server_id.as_str())
            .ok_or_else(|| format!("unknown MCP server: {}", item.server_id))?;

        call_tool(endpoint.as_str(), item.remote_tool_name.as_str(), arguments).await
    }

    pub(crate) async fn resolve_visibility(
        &self,
        concept_graph: &dyn ConceptGraphStore,
        threshold: f32,
    ) -> Vec<McpToolVisibility> {
        let concept_keys = self
            .tools_by_runtime
            .values()
            .map(|item| item.concept_key.clone())
            .collect::<Vec<_>>();
        let scores = concept_graph
            .concept_activation(&concept_keys)
            .await
            .unwrap_or_default();
        let threshold = (threshold as f64).clamp(0.0, 1.0);

        self.tools_by_runtime
            .values()
            .map(|item| {
                let score = scores
                    .get(item.concept_key.as_str())
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                let visible = score >= threshold;
                McpToolVisibility {
                    runtime_tool_name: item.runtime_tool_name.clone(),
                    concept_key: item.concept_key.clone(),
                    score,
                    visible,
                    reason: if visible {
                        "activation_above_soft_threshold".to_string()
                    } else {
                        "activation_below_soft_threshold".to_string()
                    },
                }
            })
            .collect()
    }
}

async fn discover_tools(url: &str) -> Result<Vec<ToolObject>, String> {
    let transport = StreamableHttpClientTransport::from_uri(url.to_string());
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "tsuki-core-rust".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ..Default::default()
        },
    };
    let client = client_info
        .serve(transport)
        .await
        .map_err(|err| format!("mcp client init failed: {}", err))?;
    let response = client
        .list_tools(None)
        .await
        .map_err(|err| format!("mcp tools/list failed: {}", err))?;
    let tools = response
        .tools
        .iter()
        .map(|item| {
            serde_json::to_value(item)
                .ok()
                .and_then(|value| serde_json::from_value::<ToolObject>(value).ok())
                .unwrap_or_else(|| ToolObject {
                    name: item.name.to_string(),
                    description: None,
                    input_schema: None,
                })
        })
        .collect::<Vec<_>>();
    let _ = client.cancel().await;
    Ok(tools)
}

async fn call_tool(url: &str, remote_tool_name: &str, arguments: Value) -> Result<String, String> {
    let transport = StreamableHttpClientTransport::from_uri(url.to_string());
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "tsuki-core-rust".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ..Default::default()
        },
    };
    let client = client_info
        .serve(transport)
        .await
        .map_err(|err| format!("mcp client init failed: {}", err))?;
    let arguments = match arguments {
        Value::Object(map) => Some(map),
        _ => {
            let mut object = serde_json::Map::new();
            object.insert("value".to_string(), arguments);
            Some(object)
        }
    };
    let response = client
        .call_tool(CallToolRequestParam {
            name: remote_tool_name.to_string().into(),
            arguments,
        })
        .await
        .map_err(|err| format!("mcp tools/call failed: {}", err))?;
    let _ = client.cancel().await;

    if let Some(value) = response.structured_content {
        return Ok(value.to_string());
    }
    if let Some(first) = response.content.first() {
        if let Some(text) = first.raw.as_text() {
            return Ok(text.text.to_string());
        }
    }
    Ok(json!({"ok": true}).to_string())
}

fn map_tool(server_id: &str, _url: &str, item: &ToolObject) -> Result<McpToolDescriptor, String> {
    let server_norm = normalize_key(server_id).ok_or_else(|| {
        format!(
            "mcp mapping failed: invalid server id for runtime naming server={}",
            server_id
        )
    })?;
    let tool_norm = normalize_key(item.name.as_str()).ok_or_else(|| {
        format!(
            "mcp mapping failed: invalid remote tool name for runtime naming server={} tool={}",
            server_id, item.name
        )
    })?;

    let runtime_tool_name = format!("{}__{}", server_norm, tool_norm);
    let concept_key = format!("mcp_tool:{}__{}", server_norm, tool_norm);
    let parameters = item.input_schema.clone().unwrap_or_else(default_parameters_schema);

    Ok(McpToolDescriptor {
        runtime_tool_name,
        server_id: server_id.to_string(),
        remote_tool_name: item.name.clone(),
        concept_key,
        description: item.description.clone(),
        parameters,
    })
}

fn normalize_key(value: &str) -> Option<String> {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' {
            out.push('_');
        } else {
            return None;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn default_parameters_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}
