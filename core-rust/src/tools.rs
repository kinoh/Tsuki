use crate::llm::{ToolError, ToolHandler};
use crate::event::build_event;
use crate::state::{StateRecord, StateStore};
use async_openai::types::responses::{FunctionTool, Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub const STATE_SET_TOOL: &str = "state_set";
pub const STATE_GET_TOOL: &str = "state_get";
pub const STATE_SEARCH_TOOL: &str = "state_search";
pub const EMIT_USER_REPLY_TOOL: &str = "emit_user_reply";

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
  ]
}

pub struct StateToolHandler {
  store: Arc<dyn StateStore>,
  emit_event: Arc<dyn Fn(crate::event::Event) + Send + Sync>,
}

impl StateToolHandler {
  pub fn new(
    store: Arc<dyn StateStore>,
    emit_event: Arc<dyn Fn(crate::event::Event) + Send + Sync>,
  ) -> Self {
    Self { store, emit_event }
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
          "internal",
          "text",
          json!({ "text": args.text, "target": "user" }),
          vec!["action".to_string(), "response".to_string()],
        );
        (self.emit_event)(event);
        Ok("{\"ok\":true}".to_string())
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
