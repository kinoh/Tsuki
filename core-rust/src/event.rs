use crate::time_utils::now_iso8601;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
  pub event_id: String,
  pub ts: String,
  pub source: String,
  pub modality: String,
  pub payload: Value,
  pub meta: EventMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
  pub tags: Vec<String>,
}

pub fn build_event(source: &str, modality: &str, payload: Value, tags: Vec<String>) -> Event {
  Event {
    event_id: Uuid::new_v4().to_string(),
    ts: now_iso8601(),
    source: source.to_string(),
    modality: modality.to_string(),
    payload,
    meta: EventMeta { tags },
  }
}
