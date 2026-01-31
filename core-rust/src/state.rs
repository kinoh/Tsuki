use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::RwLock;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRecord {
  pub key: String,
  pub content: String,
  pub related_keys: Vec<String>,
  pub metadata: Value,
  pub updated_at: String,
}

pub trait StateStore: Send + Sync {
  fn set(&self, key: String, content: String, related_keys: Vec<String>, metadata: Value) -> StateRecord;
  fn get(&self, key: &str) -> Option<StateRecord>;
  fn search(&self, query: &str, limit: usize) -> Vec<StateRecord>;
}

pub struct InMemoryStateStore {
  records: RwLock<HashMap<String, StateRecord>>,
}

impl InMemoryStateStore {
  pub fn new() -> Self {
    Self {
      records: RwLock::new(HashMap::new()),
    }
  }
}

impl StateStore for InMemoryStateStore {
  fn set(&self, key: String, content: String, related_keys: Vec<String>, metadata: Value) -> StateRecord {
    let record = StateRecord {
      key: key.clone(),
      content,
      related_keys,
      metadata,
      updated_at: now_iso8601(),
    };
    if let Ok(mut records) = self.records.write() {
      records.insert(key, record.clone());
    }
    record
  }

  fn get(&self, key: &str) -> Option<StateRecord> {
    self.records.read().ok().and_then(|records| records.get(key).cloned())
  }

  fn search(&self, query: &str, limit: usize) -> Vec<StateRecord> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
      return Vec::new();
    }
    let needle = trimmed.to_lowercase();
    self
      .records
      .read()
      .map(|records| {
        records
          .values()
          .filter(|record| {
            record.key.to_lowercase().contains(&needle)
              || record.content.to_lowercase().contains(&needle)
              || record.related_keys.iter().any(|key| key.to_lowercase().contains(&needle))
          })
          .take(limit)
          .cloned()
          .collect::<Vec<_>>()
      })
      .unwrap_or_default()
  }
}

fn now_iso8601() -> String {
  OffsetDateTime::now_utc()
    .format(&Rfc3339)
    .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
