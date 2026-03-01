use crate::activation_concept_graph::ConceptGraphStore;
use crate::event::build_event;
use crate::llm::{ToolError, ToolHandler};
use crate::scheduler::{ScheduleStore, ScheduleUpsertInput, SCHEDULE_SCOPE_DEFAULT};
use crate::state::{StateRecord, StateStore};
use async_openai::types::responses::{FunctionTool, Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::runtime::Handle;

pub const STATE_SET_TOOL: &str = "state_set";
pub const STATE_GET_TOOL: &str = "state_get";
pub const STATE_SEARCH_TOOL: &str = "state_search";
pub const EMIT_USER_REPLY_TOOL: &str = "emit_user_reply";
pub const CONCEPT_SEARCH_TOOL: &str = "concept_search";
pub const RECALL_QUERY_TOOL: &str = "recall_query";
pub const SCHEDULE_UPSERT_TOOL: &str = "schedule_upsert";
pub const SCHEDULE_LIST_TOOL: &str = "schedule_list";
pub const SCHEDULE_REMOVE_TOOL: &str = "schedule_remove";

pub fn state_tools() -> Vec<Tool> {
    vec![
        Tool::Function(FunctionTool {
            name: STATE_SET_TOOL.to_string(),
            description: Some("Set a state record by key.".to_string()),
            parameters: Some(state_set_schema()),
            strict: Some(true),
        }),
        Tool::Function(FunctionTool {
            name: STATE_GET_TOOL.to_string(),
            description: Some("Get a state record by key.".to_string()),
            parameters: Some(state_get_schema()),
            strict: Some(true),
        }),
        Tool::Function(FunctionTool {
            name: STATE_SEARCH_TOOL.to_string(),
            description: Some("Search state records by query.".to_string()),
            parameters: Some(state_search_schema()),
            strict: Some(true),
        }),
        Tool::Function(FunctionTool {
            name: EMIT_USER_REPLY_TOOL.to_string(),
            description: Some("Emit a reply message to the user.".to_string()),
            parameters: Some(emit_user_reply_schema()),
            strict: Some(true),
        }),
        Tool::Function(FunctionTool {
            name: SCHEDULE_UPSERT_TOOL.to_string(),
            description: Some("Create or update a schedule.".to_string()),
            parameters: Some(schedule_upsert_schema()),
            strict: Some(true),
        }),
        Tool::Function(FunctionTool {
            name: SCHEDULE_LIST_TOOL.to_string(),
            description: Some("List schedules.".to_string()),
            parameters: Some(schedule_list_schema()),
            strict: Some(true),
        }),
        Tool::Function(FunctionTool {
            name: SCHEDULE_REMOVE_TOOL.to_string(),
            description: Some("Remove a schedule by id.".to_string()),
            parameters: Some(schedule_remove_schema()),
            strict: Some(true),
        }),
    ]
}

pub fn concept_graph_tools(include_search: bool, include_recall: bool) -> Vec<Tool> {
    let mut tools = Vec::new();
    if include_search {
        tools.push(Tool::Function(FunctionTool {
            name: CONCEPT_SEARCH_TOOL.to_string(),
            description: Some(
                "Search existing concept graph nodes by simple query terms.".to_string(),
            ),
            parameters: Some(concept_search_schema()),
            strict: Some(true),
        }));
    }
    if include_recall {
        tools.push(Tool::Function(FunctionTool {
            name: RECALL_QUERY_TOOL.to_string(),
            description: Some(
                "Recall related concept graph information from seed nodes.".to_string(),
            ),
            parameters: Some(recall_query_schema()),
            strict: Some(true),
        }));
    }
    tools
}

pub struct StateToolHandler {
    store: Arc<dyn StateStore>,
    concept_graph: Arc<dyn ConceptGraphStore>,
    schedule_store: Arc<ScheduleStore>,
    emit_event: Arc<dyn Fn(crate::event::Event) + Send + Sync>,
}

impl StateToolHandler {
    pub fn new(
        store: Arc<dyn StateStore>,
        concept_graph: Arc<dyn ConceptGraphStore>,
        schedule_store: Arc<ScheduleStore>,
        emit_event: Arc<dyn Fn(crate::event::Event) + Send + Sync>,
    ) -> Self {
        Self {
            store,
            concept_graph,
            schedule_store,
            emit_event,
        }
    }
}

impl ToolHandler for StateToolHandler {
    fn handle(&self, tool_name: &str, arguments: &str) -> Result<String, ToolError> {
        match tool_name {
            STATE_SET_TOOL => {
                let args: StateSetArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let record = self.store.set(
                    args.key,
                    args.content,
                    args.related_keys.unwrap_or_default(),
                    args.metadata.unwrap_or_else(|| json!({})),
                );
                Ok(to_json_string(&record))
            }
            STATE_GET_TOOL => {
                let args: StateGetArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let result = self.store.get(&args.key);
                Ok(to_json_string(&state_get_result(result)))
            }
            STATE_SEARCH_TOOL => {
                let args: StateSearchArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let limit = args.limit.unwrap_or(5).min(50);
                let results = self.store.search(&args.query, limit);
                Ok(to_json_string(&state_search_result(results)))
            }
            EMIT_USER_REPLY_TOOL => {
                let args: EmitUserReplyArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let event = build_event(
                    "assistant",
                    "text",
                    json!({ "text": args.text }),
                    vec!["response".to_string()],
                );
                (self.emit_event)(event);
                Ok("{\"ok\":true}".to_string())
            }
            CONCEPT_SEARCH_TOOL => {
                let args: ConceptSearchArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let limit = args.limit.max(1).min(200);
                let query_terms = args.query_terms;
                let concepts = tokio::task::block_in_place(|| {
                    Handle::current()
                        .block_on(self.concept_graph.concept_search(&query_terms, limit))
                })
                .map_err(ToolError::new)?;
                Ok(to_json_string(&json!({
                    "active_concepts_from_concept_graph": concepts
                })))
            }
            RECALL_QUERY_TOOL => {
                let args: RecallQueryArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let max_hop = args.max_hop.max(1).min(8);
                let seeds = args.seeds;
                let value = tokio::task::block_in_place(|| {
                    Handle::current().block_on(self.concept_graph.recall_query(seeds, max_hop))
                })
                .map_err(ToolError::new)?;
                Ok(to_json_string(&value))
            }
            SCHEDULE_UPSERT_TOOL => {
                let args: ScheduleUpsertArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let input = ScheduleUpsertInput {
                    id: args.id,
                    recurrence: args.recurrence,
                    timezone: args.timezone,
                    action: args.action,
                    enabled: args.enabled,
                };
                let schedule = tokio::task::block_in_place(|| {
                    Handle::current()
                        .block_on(self.schedule_store.upsert(SCHEDULE_SCOPE_DEFAULT, input))
                })
                .map_err(ToolError::new)?;
                Ok(to_json_string(&schedule))
            }
            SCHEDULE_LIST_TOOL => {
                let _args: ScheduleListArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let schedules = tokio::task::block_in_place(|| {
                    Handle::current().block_on(self.schedule_store.list(SCHEDULE_SCOPE_DEFAULT))
                })
                .map_err(ToolError::new)?;
                Ok(to_json_string(&json!({
                    "count": schedules.len(),
                    "items": schedules,
                })))
            }
            SCHEDULE_REMOVE_TOOL => {
                let args: ScheduleRemoveArgs = serde_json::from_str(arguments)
                    .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
                let removed = tokio::task::block_in_place(|| {
                    Handle::current().block_on(
                        self.schedule_store
                            .remove(SCHEDULE_SCOPE_DEFAULT, args.id.as_str()),
                    )
                })
                .map_err(ToolError::new)?;
                Ok(to_json_string(&json!({
                    "ok": true,
                    "removed": removed,
                })))
            }
            _ => Err(ToolError::new(format!("unknown tool: {}", tool_name))),
        }
    }
}

#[derive(Debug, Deserialize)]
struct StateSetArgs {
    key: String,
    content: String,
    related_keys: Option<Vec<String>>,
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct StateGetArgs {
    key: String,
}

#[derive(Debug, Deserialize)]
struct StateSearchArgs {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct EmitUserReplyArgs {
    text: String,
}

#[derive(Debug, Deserialize)]
struct ConceptSearchArgs {
    query_terms: Vec<String>,
    limit: usize,
}

#[derive(Debug, Deserialize)]
struct RecallQueryArgs {
    seeds: Vec<String>,
    max_hop: u32,
}

#[derive(Debug, Deserialize)]
struct ScheduleUpsertArgs {
    id: String,
    recurrence: crate::scheduler::ScheduleRecurrence,
    timezone: String,
    action: crate::scheduler::ScheduleAction,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct ScheduleListArgs {}

#[derive(Debug, Deserialize)]
struct ScheduleRemoveArgs {
    id: String,
}

fn state_get_result(record: Option<StateRecord>) -> Value {
    match record {
        Some(value) => json!({ "found": true, "record": value }),
        None => json!({ "found": false }),
    }
}

fn state_search_result(results: Vec<StateRecord>) -> Value {
    json!({
      "count": results.len(),
      "results": results,
    })
}

fn to_json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{\"error\":\"serialization\"}".to_string())
}

fn state_set_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "key": { "type": "string" },
        "content": { "type": "string" },
        "related_keys": { "type": "array", "items": { "type": "string" } },
        "metadata": { "type": "object", "properties": {}, "additionalProperties": false }
      },
      "required": ["key", "content", "related_keys", "metadata"],
      "additionalProperties": false
    })
}

fn state_get_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "key": { "type": "string" }
      },
      "required": ["key"],
      "additionalProperties": false
    })
}

fn state_search_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "query": { "type": "string" },
        "limit": { "type": "integer", "minimum": 1 }
      },
      "required": ["query", "limit"],
      "additionalProperties": false
    })
}

fn emit_user_reply_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "text": { "type": "string" }
      },
      "required": ["text"],
      "additionalProperties": false
    })
}

fn schedule_upsert_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "id": { "type": "string" },
        "recurrence": {
          "type": "object",
          "oneOf": [
            {
              "type": "object",
              "properties": {
                "kind": { "type": "string", "const": "once" },
                "at": { "type": "string" }
              },
              "required": ["kind", "at"],
              "additionalProperties": false
            },
            {
              "type": "object",
              "properties": {
                "kind": { "type": "string", "const": "daily" },
                "at": { "type": "string" }
              },
              "required": ["kind", "at"],
              "additionalProperties": false
            },
            {
              "type": "object",
              "properties": {
                "kind": { "type": "string", "const": "weekly" },
                "weekdays": { "type": "array", "items": { "type": "integer" }, "minItems": 1 },
                "at": { "type": "string" }
              },
              "required": ["kind", "weekdays", "at"],
              "additionalProperties": false
            },
            {
              "type": "object",
              "properties": {
                "kind": { "type": "string", "const": "monthly" },
                "day": { "type": "integer", "minimum": 1, "maximum": 31 },
                "at": { "type": "string" }
              },
              "required": ["kind", "day", "at"],
              "additionalProperties": false
            },
            {
              "type": "object",
              "properties": {
                "kind": { "type": "string", "const": "interval" },
                "seconds": { "type": "integer", "minimum": 1 }
              },
              "required": ["kind", "seconds"],
              "additionalProperties": false
            }
          ]
        },
        "timezone": { "type": "string" },
        "action": {
          "type": "object",
          "properties": {
            "kind": { "type": "string", "const": "emit_event" },
            "event": { "type": "string" },
            "payload": { "type": "object" }
          },
          "required": ["kind", "event", "payload"],
          "additionalProperties": false
        },
        "enabled": { "type": "boolean" }
      },
      "required": ["id", "recurrence", "timezone", "action", "enabled"],
      "additionalProperties": false
    })
}

fn schedule_list_schema() -> Value {
    json!({
      "type": "object",
      "properties": {},
      "required": [],
      "additionalProperties": false
    })
}

fn schedule_remove_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "id": { "type": "string" }
      },
      "required": ["id"],
      "additionalProperties": false
    })
}

fn concept_search_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "query_terms": { "type": "array", "items": { "type": "string" } },
        "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
      },
      "required": ["query_terms", "limit"],
      "additionalProperties": false
    })
}

fn recall_query_schema() -> Value {
    json!({
      "type": "object",
      "properties": {
        "seeds": { "type": "array", "items": { "type": "string" } },
        "max_hop": { "type": "integer", "minimum": 1, "maximum": 8 }
      },
      "required": ["seeds", "max_hop"],
      "additionalProperties": false
    })
}
