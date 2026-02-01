use crate::event::Event;
use crate::time_utils::now_iso8601;
use libsql::{params, Connection};
use serde_json::Value;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type DbResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct Db {
  conn: Mutex<Connection>,
}

impl Db {
  pub async fn connect() -> DbResult<Arc<Self>> {
    let db_path = std::env::var("DB_PATH").unwrap_or_else(|_| "./data/core-rust.db".to_string());
    if let Some(parent) = Path::new(&db_path).parent() {
      if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)?;
      }
    }

    let db = if let Ok(url) = std::env::var("TURSO_DATABASE_URL") {
      let token = std::env::var("TURSO_AUTH_TOKEN")
        .map_err(|_| "TURSO_AUTH_TOKEN is required for remote databases")?;
      libsql::Builder::new_remote_replica(&db_path, url, token)
        .build()
        .await?
    } else {
      libsql::Builder::new_local(&db_path).build().await?
    };

    let conn = db.connect()?;
    let db = Arc::new(Self {
      conn: Mutex::new(conn),
    });
    db.init_schema().await?;
    Ok(db)
  }

  async fn init_schema(&self) -> DbResult<()> {
    let conn = self.conn.lock().await;
    conn
      .execute(
        "CREATE TABLE IF NOT EXISTS events (\
        event_id TEXT PRIMARY KEY,\
        ts TEXT NOT NULL,\
        source TEXT NOT NULL,\
        modality TEXT NOT NULL,\
        payload_json TEXT NOT NULL,\
        tags_json TEXT NOT NULL\
      )",
        params![],
      )
      .await?;
    conn
      .execute(
        "CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts)",
        params![],
      )
      .await?;
    conn
      .execute(
        "CREATE TABLE IF NOT EXISTS state_records (\
        key TEXT PRIMARY KEY,\
        content TEXT NOT NULL,\
        related_keys_json TEXT NOT NULL,\
        metadata_json TEXT NOT NULL,\
        updated_at TEXT NOT NULL\
      )",
        params![],
      )
      .await?;
    conn
      .execute(
        "CREATE TABLE IF NOT EXISTS modules (\
        name TEXT PRIMARY KEY,\
        instructions TEXT NOT NULL,\
        enabled INTEGER NOT NULL,\
        updated_at TEXT NOT NULL\
      )",
        params![],
      )
      .await?;
    Ok(())
  }

  pub async fn insert_event(&self, event: &Event) -> DbResult<()> {
    let payload_json = serde_json::to_string(&event.payload)?;
    let tags_json = serde_json::to_string(&event.meta.tags)?;
    let conn = self.conn.lock().await;
    conn
      .execute(
        "INSERT INTO events (event_id, ts, source, modality, payload_json, tags_json) VALUES (?, ?, ?, ?, ?, ?)",
        params![
          event.event_id.as_str(),
          event.ts.as_str(),
          event.source.as_str(),
          event.modality.as_str(),
          payload_json,
          tags_json
        ],
      )
      .await?;
    Ok(())
  }

  pub async fn load_latest_events(&self, limit: usize) -> DbResult<Vec<Event>> {
    if limit == 0 {
      return Ok(Vec::new());
    }
    let conn = self.conn.lock().await;
    let mut rows = conn
      .query(
        "SELECT event_id, ts, source, modality, payload_json, tags_json FROM events ORDER BY ts DESC LIMIT ?",
        params![limit as i64],
      )
      .await?;
    let mut events = Vec::new();
    while let Some(row) = rows.next().await? {
      let event_id: String = row.get(0)?;
      let ts: String = row.get(1)?;
      let source: String = row.get(2)?;
      let modality: String = row.get(3)?;
      let payload_json: String = row.get(4)?;
      let tags_json: String = row.get(5)?;
      let payload: Value = serde_json::from_str(&payload_json)?;
      let tags: Vec<String> = serde_json::from_str(&tags_json)?;
      events.push(Event {
        event_id,
        ts,
        source,
        modality,
        payload,
        meta: crate::event::EventMeta { tags },
      });
    }
    events.reverse();
    Ok(events)
  }

  pub async fn upsert_state_record(
    &self,
    key: &str,
    content: &str,
    related_keys_json: &str,
    metadata_json: &str,
  ) -> DbResult<String> {
    let updated_at = now_iso8601();
    let conn = self.conn.lock().await;
    conn
      .execute(
        "INSERT INTO state_records (key, content, related_keys_json, metadata_json, updated_at)\
         VALUES (?, ?, ?, ?, ?)\
         ON CONFLICT(key) DO UPDATE SET\
           content=excluded.content,\
           related_keys_json=excluded.related_keys_json,\
           metadata_json=excluded.metadata_json,\
           updated_at=excluded.updated_at",
        params![key, content, related_keys_json, metadata_json, updated_at.clone()],
      )
      .await?;
    Ok(updated_at)
  }

  pub async fn get_state_record(&self, key: &str) -> DbResult<Option<(String, String, String, String)>> {
    let conn = self.conn.lock().await;
    let mut rows = conn
      .query(
        "SELECT content, related_keys_json, metadata_json, updated_at FROM state_records WHERE key = ?",
        params![key],
      )
      .await?;
    if let Some(row) = rows.next().await? {
      let content: String = row.get(0)?;
      let related_keys_json: String = row.get(1)?;
      let metadata_json: String = row.get(2)?;
      let updated_at: String = row.get(3)?;
      Ok(Some((content, related_keys_json, metadata_json, updated_at)))
    } else {
      Ok(None)
    }
  }

  pub async fn search_state_records(&self, query: &str, limit: usize) -> DbResult<Vec<(String, String, String, String, String)>> {
    let q = format!("%{}%", query);
    let conn = self.conn.lock().await;
    let mut rows = conn
      .query(
        "SELECT key, content, related_keys_json, metadata_json, updated_at FROM state_records WHERE\
         key LIKE ? OR content LIKE ? OR related_keys_json LIKE ?\
         ORDER BY updated_at DESC LIMIT ?",
        params![q.clone(), q.clone(), q, limit as i64],
      )
      .await?;
    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
      let key: String = row.get(0)?;
      let content: String = row.get(1)?;
      let related_keys_json: String = row.get(2)?;
      let metadata_json: String = row.get(3)?;
      let updated_at: String = row.get(4)?;
      results.push((key, content, related_keys_json, metadata_json, updated_at));
    }
    Ok(results)
  }

  pub async fn upsert_module(&self, name: &str, instructions: &str, enabled: bool) -> DbResult<()> {
    let updated_at = now_iso8601();
    let conn = self.conn.lock().await;
    conn
      .execute(
        "INSERT INTO modules (name, instructions, enabled, updated_at) VALUES (?, ?, ?, ?)\
         ON CONFLICT(name) DO UPDATE SET\
           instructions=excluded.instructions,\
           enabled=excluded.enabled,\
           updated_at=excluded.updated_at",
        params![name, instructions, if enabled { 1 } else { 0 }, updated_at],
      )
      .await?;
    Ok(())
  }

  pub async fn list_active_modules(&self) -> DbResult<Vec<(String, String, bool)>> {
    let conn = self.conn.lock().await;
    let mut rows = conn
      .query(
        "SELECT name, instructions, enabled FROM modules WHERE enabled = 1 ORDER BY name",
        params![],
      )
      .await?;
    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
      let name: String = row.get(0)?;
      let instructions: String = row.get(1)?;
      let enabled: i64 = row.get(2)?;
      results.push((name, instructions, enabled != 0));
    }
    Ok(results)
  }
}
