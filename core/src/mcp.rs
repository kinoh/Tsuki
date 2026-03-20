use async_openai::types::responses::{FunctionTool, Tool};
use rmcp::model::{CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation};
use rmcp::service::ServiceExt;
use rmcp::transport::StreamableHttpClientTransport;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::activation_concept_graph::ConceptGraphStore;
use crate::config::{InternalPromptConfig, McpServerConfig};
use crate::event::contracts::{llm_error, llm_raw};
use crate::event::Event;
use crate::llm::{LlmAdapter, LlmRequest};
use crate::mcp_trigger_concepts::{
    build_trigger_concept_prompts, parse_trigger_concepts, TriggerConceptExtractionInput,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct McpToolDescriptor {
    pub(crate) runtime_tool_name: String,
    pub(crate) server_id: String,
    pub(crate) remote_tool_name: String,
    pub(crate) concept_key: String,
    pub(crate) description: Option<String>,
    pub(crate) input_schema: Value,
    pub(crate) llm_parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub(crate) trigger_associations: Vec<McpTriggerAssociationLog>,
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct McpTriggerAssociationLog {
    pub(crate) server_id: String,
    pub(crate) tool_name: String,
    pub(crate) tool_concept: String,
    pub(crate) trigger_concepts: Vec<String>,
    pub(crate) relation_success_count: usize,
}

#[derive(Debug, Deserialize)]
struct ToolObject {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    #[serde(alias = "inputSchema")]
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
        llm: Arc<dyn LlmAdapter>,
        internal_prompts: &InternalPromptConfig,
        emit_event: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> McpBootstrapResult {
        let mut out = Self::empty();
        let mut auto_created = Vec::<McpAutoCreatedLog>::new();
        let mut trigger_associations = Vec::<McpTriggerAssociationLog>::new();
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

                if let Some(existing) = out.tools_by_runtime.get(mapping.runtime_tool_name.as_str())
                {
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

                // skill_* tools: register for call_tool but skip concept graph processing.
                if discovered_tool.name.starts_with("skill_") {
                    out.tools_by_runtime
                        .insert(mapping.runtime_tool_name.clone(), mapping);
                    continue;
                }

                if !concept_keys.insert(mapping.concept_key.clone()) {
                    errors.push(format!(
                        "mcp mapping failed: concept key collision concept_key={} server={} remote_tool={}",
                        mapping.concept_key, mapping.server_id, mapping.remote_tool_name
                    ));
                    continue;
                }

                let concept_upsert = upsert_result_label(
                    concept_graph
                        .concept_upsert(mapping.concept_key.clone())
                        .await,
                );
                auto_created.push(McpAutoCreatedLog {
                    server_id: mapping.server_id.clone(),
                    tool_name: mapping.remote_tool_name.clone(),
                    concept_key: mapping.concept_key.clone(),
                    reason: "missing_concept",
                    result: concept_upsert,
                    phase: "bootstrap",
                });
                out.tools_by_runtime
                    .insert(mapping.runtime_tool_name.clone(), mapping.clone());
                let extracted = match extract_trigger_concepts_with_llm(
                    llm.clone(),
                    internal_prompts,
                    mapping.server_id.as_str(),
                    &discovered_tool,
                    emit_event.clone(),
                )
                .await
                {
                    Ok(values) => values,
                    Err(err) => {
                        errors.push(format!(
                            "mcp trigger onboarding failed server={} tool={} stage=parse_or_non_empty error={}",
                            mapping.server_id, mapping.remote_tool_name, err
                        ));
                        continue;
                    }
                };
                let mut relation_success_count = 0usize;
                for trigger in &extracted {
                    let _ = concept_graph.concept_upsert(trigger.clone()).await;
                    match concept_graph
                        .relation_add(
                            trigger.clone(),
                            mapping.concept_key.clone(),
                            "evokes".to_string(),
                        )
                        .await
                    {
                        Ok(_) => relation_success_count += 1,
                        Err(err) => {
                            errors.push(format!(
                                "mcp trigger onboarding failed server={} tool={} stage=edge trigger={} error={}",
                                mapping.server_id, mapping.remote_tool_name, trigger, err
                            ));
                        }
                    }
                }
                if relation_success_count == 0 {
                    errors.push(format!(
                        "mcp trigger onboarding failed server={} tool={} stage=edge error=no_relation_created",
                        mapping.server_id, mapping.remote_tool_name
                    ));
                    continue;
                }
                trigger_associations.push(McpTriggerAssociationLog {
                    server_id: mapping.server_id.clone(),
                    tool_name: mapping.remote_tool_name.clone(),
                    tool_concept: mapping.concept_key.clone(),
                    trigger_concepts: extracted,
                    relation_success_count,
                });
            }
        }

        McpBootstrapResult {
            registry: out,
            auto_created,
            trigger_associations,
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
                    parameters: Some(item.llm_parameters.clone()),
                    strict: Some(true),
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

    pub(crate) fn validate_call_arguments(
        &self,
        runtime_tool_name: &str,
        arguments: &Value,
    ) -> Result<(), String> {
        let item = self
            .tools_by_runtime
            .get(runtime_tool_name)
            .ok_or_else(|| format!("unknown MCP runtime tool: {}", runtime_tool_name))?;
        let object = arguments.as_object().ok_or_else(|| {
            format!(
                "invalid arguments for {}: expected JSON object",
                runtime_tool_name
            )
        })?;
        let required = item
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let missing = required
            .iter()
            .filter(|name| !object.contains_key(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(());
        }
        let mut message = format!(
            "missing required arguments for {}: {}",
            runtime_tool_name,
            missing.join(", ")
        );
        if let Some(example) = build_required_args_example(&item.input_schema) {
            message.push_str(format!(". Example: {}", example).as_str());
        }
        Err(message)
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

async fn extract_trigger_concepts_with_llm(
    llm: Arc<dyn LlmAdapter>,
    internal_prompts: &InternalPromptConfig,
    server_id: &str,
    tool: &ToolObject,
    emit_event: Arc<dyn Fn(Event) + Send + Sync>,
) -> Result<Vec<String>, String> {
    let schema = tool.input_schema.clone().unwrap_or_else(|| json!({}));
    let prompts = build_trigger_concept_prompts(
        &TriggerConceptExtractionInput {
            server_id,
            tool_name: tool.name.as_str(),
            description: tool.description.as_deref(),
            input_schema: &schema,
        },
        internal_prompts
            .mcp_trigger_extract_prompt_template
            .as_str(),
        internal_prompts
            .mcp_trigger_extract_retry_prompt_template
            .as_str(),
    );
    let mut last_error = "llm parse check failed: empty output".to_string();

    for prompt in prompts {
        let response = llm
            .respond(LlmRequest {
                input: prompt.clone(),
            })
            .await;
        let response = match response {
            Ok(value) => value,
            Err(err) => {
                let detail = format!("llm call failed: {}", err);
                emit_mcp_llm_error(
                    emit_event.clone(),
                    server_id,
                    tool.name.as_str(),
                    prompt.as_str(),
                    detail.as_str(),
                );
                return Err(detail);
            }
        };
        emit_mcp_llm_raw(
            emit_event.clone(),
            server_id,
            tool.name.as_str(),
            prompt.as_str(),
            &response.raw,
            response.text.as_str(),
        );
        let candidates = collect_trigger_output_candidates(&response);
        if candidates.is_empty() {
            last_error = "llm parse check failed: empty output".to_string();
            continue;
        }
        for text in candidates {
            match parse_trigger_concepts(text.as_str()) {
                Ok(values) => return Ok(values),
                Err(err) => {
                    last_error = err;
                }
            }
        }
    }
    Err(last_error)
}

fn emit_mcp_llm_raw(
    emit_event: Arc<dyn Fn(Event) + Send + Sync>,
    server_id: &str,
    tool_name: &str,
    prompt: &str,
    raw: &Value,
    output_text: &str,
) {
    let event = llm_raw(
        "tooling",
        json!({
            "mode": "bootstrap",
            "purpose": "mcp_trigger_concept_extraction",
            "server_id": server_id,
            "tool_name": tool_name,
            "context": prompt,
            "raw": raw,
            "output_text": output_text,
            "tool_calls": [],
        }),
        vec![
            "mode:bootstrap".to_string(),
            "purpose:mcp_trigger_concept_extraction".to_string(),
            format!("server:{}", server_id),
            format!("tool:{}", tool_name),
        ],
    );
    emit_event(event);
}

fn emit_mcp_llm_error(
    emit_event: Arc<dyn Fn(Event) + Send + Sync>,
    server_id: &str,
    tool_name: &str,
    prompt: &str,
    error: &str,
) {
    let event = llm_error(
        "tooling",
        json!({
            "mode": "bootstrap",
            "purpose": "mcp_trigger_concept_extraction",
            "server_id": server_id,
            "tool_name": tool_name,
            "context": prompt,
            "error": error,
        }),
        vec![
            "mode:bootstrap".to_string(),
            "purpose:mcp_trigger_concept_extraction".to_string(),
            format!("server:{}", server_id),
            format!("tool:{}", tool_name),
        ],
    );
    emit_event(event);
}

fn collect_trigger_output_candidates(response: &crate::llm::LlmResponse) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut seen = BTreeSet::<String>::new();
    for candidate in [
        normalize_llm_output_candidate(response.text.as_str()),
        extract_output_text_from_raw_json(&response.raw),
    ]
    .into_iter()
    .flatten()
    {
        if seen.insert(candidate.clone()) {
            out.push(candidate);
        }
    }
    out
}

fn normalize_llm_output_candidate(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "(empty response)" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn extract_output_text_from_raw_json(value: &Value) -> Option<String> {
    let output = value.get("output")?.as_array()?;
    for item in output {
        let content = item.get("content").and_then(Value::as_array);
        let Some(content) = content else {
            continue;
        };
        for chunk in content {
            if let Some(text) = chunk.get("text").and_then(Value::as_str) {
                if let Some(candidate) = normalize_llm_output_candidate(text) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn upsert_result_label(result: Result<Value, String>) -> &'static str {
    match result {
        Ok(value) => {
            let created = value
                .get("created")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if created {
                "created"
            } else {
                "already_exists"
            }
        }
        Err(_) => "already_exists",
    }
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
    let input_schema = item
        .input_schema
        .clone()
        .unwrap_or_else(default_parameters_schema);
    let llm_parameters = normalize_function_parameters_for_openai(input_schema.clone());

    Ok(McpToolDescriptor {
        runtime_tool_name,
        server_id: server_id.to_string(),
        remote_tool_name: item.name.clone(),
        concept_key,
        description: item.description.clone(),
        input_schema,
        llm_parameters,
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
        "additionalProperties": false
    })
}

fn normalize_function_parameters_for_openai(mut value: Value) -> Value {
    normalize_openai_schema_node(&mut value);
    value
}

fn normalize_openai_schema_node(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    if matches!(object.get("type").and_then(Value::as_str), Some("object")) {
        object
            .entry("additionalProperties".to_string())
            .or_insert(Value::Bool(false));
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            let property_names = properties.keys().cloned().collect::<Vec<_>>();
            for child in properties.values_mut() {
                normalize_openai_schema_node(child);
            }
            for name in &property_names {
                if let Some(child) = properties.get_mut(name) {
                    make_schema_nullable(child);
                }
            }
            object.insert(
                "required".to_string(),
                Value::Array(property_names.into_iter().map(Value::String).collect()),
            );
        }
    }

    if let Some(items) = object.get_mut("items") {
        normalize_openai_schema_node(items);
    }

    if let Some(any_of) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
        for child in any_of {
            normalize_openai_schema_node(child);
        }
    }
    if let Some(one_of) = object.get_mut("oneOf").and_then(Value::as_array_mut) {
        for child in one_of {
            normalize_openai_schema_node(child);
        }
    }
    if let Some(all_of) = object.get_mut("allOf").and_then(Value::as_array_mut) {
        for child in all_of {
            normalize_openai_schema_node(child);
        }
    }
}

fn make_schema_nullable(value: &mut Value) {
    let already_nullable = value
        .get("type")
        .and_then(Value::as_array)
        .map(|items| items.iter().any(|item| item.as_str() == Some("null")))
        .unwrap_or(false)
        || value
            .get("anyOf")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .any(|item| item.get("type").and_then(Value::as_str) == Some("null"))
            })
            .unwrap_or(false)
        || value.get("type").and_then(Value::as_str) == Some("null");
    if already_nullable {
        return;
    }

    let wrapped = value.clone();
    *value = json!({
        "anyOf": [
            wrapped,
            { "type": "null" }
        ]
    });
}

fn build_required_args_example(schema: &Value) -> Option<String> {
    let required = schema.get("required")?.as_array()?;
    let properties = schema.get("properties")?.as_object()?;
    let mut example = serde_json::Map::new();
    for name in required.iter().filter_map(Value::as_str) {
        let property = properties.get(name)?;
        example.insert(name.to_string(), example_value_for_schema(property, name));
    }
    if example.is_empty() {
        None
    } else {
        serde_json::to_string(&Value::Object(example)).ok()
    }
}

fn example_value_for_schema(schema: &Value, fallback_name: &str) -> Value {
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => Value::String(example_string_for_schema(schema, fallback_name)),
        Some("integer") => Value::Number(1.into()),
        Some("number") => json!(1),
        Some("boolean") => Value::Bool(false),
        Some("array") => Value::Array(vec![]),
        Some("object") => Value::Object(serde_json::Map::new()),
        _ => Value::String(format!("<{}>", fallback_name)),
    }
}

fn example_string_for_schema(schema: &Value, fallback_name: &str) -> String {
    let description = schema
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or(fallback_name)
        .to_ascii_lowercase();
    if description.contains("command") || fallback_name == "command" {
        "<command>".to_string()
    } else if description.contains("path") {
        "<path>".to_string()
    } else if description.contains("url") {
        "<url>".to_string()
    } else if description.contains("id") || fallback_name == "id" {
        "<id>".to_string()
    } else {
        format!("<{}>", fallback_name)
    }
}
