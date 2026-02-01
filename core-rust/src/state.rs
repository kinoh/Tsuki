use crate::db::Db;
use crate::time_utils::now_iso8601;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::task::block_in_place;

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

pub struct DbStateStore {
  db: Arc<Db>,
}

impl DbStateStore {
  pub fn new(db: Arc<Db>) -> Self {
    Self { db }
  }
}

impl StateStore for DbStateStore {
  fn set(&self, key: String, content: String, related_keys: Vec<String>, metadata: Value) -> StateRecord {
    let related_keys_json = serde_json::to_string(&related_keys).unwrap_or_else(|_| "[]".to_string());
    let metadata_json = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());
    let db = self.db.clone();
    let updated_at = block_in_place(|| {
      Handle::current().block_on(async {
        db
          .upsert_state_record(&key, &content, &related_keys_json, &metadata_json)
          .await
      })
    })
    .unwrap_or_else(|_| now_iso8601());

    StateRecord {
      key,
      content,
      related_keys,
      metadata,
      updated_at,
    }
  }

  fn get(&self, key: &str) -> Option<StateRecord> {
    let db = self.db.clone();
    let result = block_in_place(|| Handle::current().block_on(async { db.get_state_record(key).await }));
    match result {
      Ok(Some((content, related_keys_json, metadata_json, updated_at))) => {
        let related_keys = serde_json::from_str(&related_keys_json).unwrap_or_default();
        let metadata = serde_json::from_str(&metadata_json).unwrap_or_else(|_| Value::Null);
        Some(StateRecord {
          key: key.to_string(),
          content,
          related_keys,
          metadata,
          updated_at,
        })
      }
      _ => None,
    }
  }

  fn search(&self, query: &str, limit: usize) -> Vec<StateRecord> {
    let trimmed = query.trim();
    if trimmed.is_empty() || limit == 0 {
      return Vec::new();
    }
    let db = self.db.clone();
    let result = block_in_place(|| {
      Handle::current().block_on(async { db.search_state_records(trimmed, limit).await })
    });
    match result {
      Ok(rows) => rows
        .into_iter()
        .map(|(key, content, related_keys_json, metadata_json, updated_at)| {
          let related_keys = serde_json::from_str(&related_keys_json).unwrap_or_default();
          let metadata = serde_json::from_str(&metadata_json).unwrap_or_else(|_| Value::Null);
          StateRecord {
            key,
            content,
            related_keys,
            metadata,
            updated_at,
          }
        })
        .collect(),
      Err(_) => Vec::new(),
    }
  }
}
