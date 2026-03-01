use crate::clock::now_iso8601;
use crate::config::DbConfig;
use crate::event::Event;
use libsql::{params, Connection};
use serde_json::Value;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type DbResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug, Clone)]
pub struct RuntimeConfigRecord {
    pub enable_notification: bool,
    pub enable_sensory: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct AdminSessionRecord {
    pub session_id: String,
    pub created_at: String,
    pub last_seen_at: String,
    pub password_fingerprint: String,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub async fn connect(config: &DbConfig) -> DbResult<Arc<Self>> {
        let db_path = config.path.clone();
        if let Some(parent) = Path::new(&db_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let db = if let Some(url) = config.remote_url.clone() {
            let token = std::env::var("TURSO_AUTH_TOKEN").map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "TURSO_AUTH_TOKEN is required for remote databases",
                )
            })?;
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
        conn.execute(
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
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts)",
            params![],
        )
        .await?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schedules (\
        scope TEXT NOT NULL,\
        id TEXT NOT NULL,\
        recurrence_json TEXT NOT NULL,\
        timezone TEXT NOT NULL,\
        action_json TEXT NOT NULL,\
        enabled INTEGER NOT NULL,\
        next_fire_at TEXT,\
        updated_at TEXT NOT NULL,\
        PRIMARY KEY (scope, id)\
      )",
            params![],
        )
        .await?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_schedules_due ON schedules(enabled, next_fire_at)",
            params![],
        )
        .await?;
        conn.execute(
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
        conn.execute(
            "CREATE TABLE IF NOT EXISTS modules (\
        name TEXT PRIMARY KEY,\
        instructions TEXT NOT NULL,\
        enabled INTEGER NOT NULL,\
        updated_at TEXT NOT NULL\
      )",
            params![],
        )
        .await?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS runtime_config (\
        id INTEGER PRIMARY KEY CHECK (id = 1),\
        enable_notification INTEGER NOT NULL,\
        enable_sensory INTEGER NOT NULL,\
        updated_at TEXT NOT NULL\
      )",
            params![],
        )
        .await?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS notification_tokens (\
        user_id TEXT NOT NULL,\
        token TEXT NOT NULL,\
        created_at TEXT NOT NULL,\
        PRIMARY KEY (user_id, token)\
      )",
            params![],
        )
        .await?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS admin_sessions (\
        session_id TEXT PRIMARY KEY,\
        created_at TEXT NOT NULL,\
        last_seen_at TEXT NOT NULL,\
        password_fingerprint TEXT NOT NULL\
      )",
            params![],
        )
        .await?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_admin_sessions_last_seen ON admin_sessions(last_seen_at)",
            params![],
        )
        .await?;
        let now = now_iso8601();
        conn.execute(
            "INSERT OR IGNORE INTO runtime_config (id, enable_notification, enable_sensory, updated_at) \
             VALUES (1, 1, 1, ?)",
            params![now],
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

    pub async fn load_events(
        &self,
        limit: usize,
        before_ts: Option<&str>,
        desc: bool,
    ) -> DbResult<Vec<Event>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().await;
        let mut rows = match (before_ts, desc) {
            (Some(ts), true) => {
                conn.query(
                    "SELECT event_id, ts, source, modality, payload_json, tags_json FROM events \
                     WHERE ts < ? \
                     ORDER BY ts DESC, event_id DESC \
                     LIMIT ?",
                    params![ts, limit as i64],
                )
                .await?
            }
            (Some(ts), false) => {
                conn.query(
                    "SELECT event_id, ts, source, modality, payload_json, tags_json FROM events \
                     WHERE ts < ? \
                     ORDER BY ts ASC, event_id ASC \
                     LIMIT ?",
                    params![ts, limit as i64],
                )
                .await?
            }
            (None, true) => {
                conn.query(
                    "SELECT event_id, ts, source, modality, payload_json, tags_json FROM events \
                     ORDER BY ts DESC, event_id DESC \
                     LIMIT ?",
                    params![limit as i64],
                )
                .await?
            }
            (None, false) => {
                conn.query(
                    "SELECT event_id, ts, source, modality, payload_json, tags_json FROM events \
                     ORDER BY ts ASC, event_id ASC \
                     LIMIT ?",
                    params![limit as i64],
                )
                .await?
            }
        };

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
        Ok(events)
    }

    pub async fn get_event_by_id(&self, event_id: &str) -> DbResult<Option<Event>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
      .query(
        "SELECT event_id, ts, source, modality, payload_json, tags_json FROM events WHERE event_id = ? LIMIT 1",
        params![event_id],
      )
      .await?;
        if let Some(row) = rows.next().await? {
            let event_id: String = row.get(0)?;
            let ts: String = row.get(1)?;
            let source: String = row.get(2)?;
            let modality: String = row.get(3)?;
            let payload_json: String = row.get(4)?;
            let tags_json: String = row.get(5)?;
            let payload: Value = serde_json::from_str(&payload_json)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json)?;
            return Ok(Some(Event {
                event_id,
                ts,
                source,
                modality,
                payload,
                meta: crate::event::EventMeta { tags },
            }));
        }
        Ok(None)
    }

    pub async fn exists_scheduler_fired(
        &self,
        schedule_id: &str,
        scheduled_at: &str,
    ) -> DbResult<bool> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT 1 FROM events \
                 WHERE tags_json LIKE '%\"scheduler.fired\"%' \
                   AND json_extract(payload_json, '$.schedule_id') = ? \
                   AND json_extract(payload_json, '$.scheduled_at') = ? \
                 LIMIT 1",
                params![schedule_id, scheduled_at],
            )
            .await?;
        Ok(rows.next().await?.is_some())
    }

    pub async fn upsert_schedule(
        &self,
        scope: &str,
        id: &str,
        recurrence_json: &str,
        timezone: &str,
        action_json: &str,
        enabled: bool,
        next_fire_at: Option<&str>,
    ) -> DbResult<String> {
        let updated_at = now_iso8601();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO schedules (scope, id, recurrence_json, timezone, action_json, enabled, next_fire_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(scope, id) DO UPDATE SET \
               recurrence_json=excluded.recurrence_json,\
               timezone=excluded.timezone,\
               action_json=excluded.action_json,\
               enabled=excluded.enabled,\
               next_fire_at=excluded.next_fire_at,\
               updated_at=excluded.updated_at",
            params![
                scope,
                id,
                recurrence_json,
                timezone,
                action_json,
                if enabled { 1 } else { 0 },
                next_fire_at,
                updated_at.clone()
            ],
        )
        .await?;
        Ok(updated_at)
    }

    pub async fn load_schedules(
        &self,
        scope: &str,
    ) -> DbResult<
        Vec<(
            String,
            String,
            String,
            String,
            String,
            i64,
            Option<String>,
            String,
        )>,
    > {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT scope, id, recurrence_json, timezone, action_json, enabled, next_fire_at, updated_at \
                 FROM schedules WHERE scope = ? ORDER BY id ASC",
                params![scope],
            )
            .await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let scope: String = row.get(0)?;
            let id: String = row.get(1)?;
            let recurrence_json: String = row.get(2)?;
            let timezone: String = row.get(3)?;
            let action_json: String = row.get(4)?;
            let enabled: i64 = row.get(5)?;
            let next_fire_at: Option<String> = row.get(6)?;
            let updated_at: String = row.get(7)?;
            results.push((
                scope,
                id,
                recurrence_json,
                timezone,
                action_json,
                enabled,
                next_fire_at,
                updated_at,
            ));
        }
        Ok(results)
    }

    pub async fn remove_schedule(&self, scope: &str, id: &str) -> DbResult<bool> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "DELETE FROM schedules WHERE scope = ? AND id = ?",
                params![scope, id],
            )
            .await?;
        Ok(affected > 0)
    }

    pub async fn acquire_due_schedules(
        &self,
        now_ts: &str,
        limit: usize,
    ) -> DbResult<Vec<(String, String, String, String, String, String)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT scope, id, recurrence_json, timezone, action_json, next_fire_at \
                 FROM schedules \
                 WHERE enabled = 1 AND next_fire_at IS NOT NULL AND next_fire_at <= ? \
                 ORDER BY next_fire_at ASC, id ASC LIMIT ?",
                params![now_ts, limit as i64],
            )
            .await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let scope: String = row.get(0)?;
            let id: String = row.get(1)?;
            let recurrence_json: String = row.get(2)?;
            let timezone: String = row.get(3)?;
            let action_json: String = row.get(4)?;
            let next_fire_at: String = row.get(5)?;
            results.push((
                scope,
                id,
                recurrence_json,
                timezone,
                action_json,
                next_fire_at,
            ));
        }
        Ok(results)
    }

    pub async fn update_schedule_next_fire(
        &self,
        scope: &str,
        id: &str,
        next_fire_at: Option<&str>,
    ) -> DbResult<()> {
        let updated_at = now_iso8601();
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE schedules SET next_fire_at = ?, updated_at = ? WHERE scope = ? AND id = ?",
            params![next_fire_at, updated_at, scope, id],
        )
        .await?;
        Ok(())
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

    pub async fn get_state_record(
        &self,
        key: &str,
    ) -> DbResult<Option<(String, String, String, String)>> {
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
            Ok(Some((
                content,
                related_keys_json,
                metadata_json,
                updated_at,
            )))
        } else {
            Ok(None)
        }
    }

    pub async fn search_state_records(
        &self,
        query: &str,
        limit: usize,
    ) -> DbResult<Vec<(String, String, String, String, String)>> {
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

    pub async fn upsert_module(
        &self,
        name: &str,
        instructions: &str,
        enabled: bool,
    ) -> DbResult<()> {
        let updated_at = now_iso8601();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO modules (name, instructions, enabled, updated_at) VALUES (?, ?, ?, ?)\
         ON CONFLICT(name) DO UPDATE SET \
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

    pub async fn get_runtime_config(&self) -> DbResult<RuntimeConfigRecord> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT enable_notification, enable_sensory, updated_at \
                 FROM runtime_config WHERE id = 1 LIMIT 1",
                params![],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let enable_notification: i64 = row.get(0)?;
            let enable_sensory: i64 = row.get(1)?;
            let updated_at: String = row.get(2)?;
            return Ok(RuntimeConfigRecord {
                enable_notification: enable_notification != 0,
                enable_sensory: enable_sensory != 0,
                updated_at,
            });
        }

        Err("runtime config row not found".into())
    }

    pub async fn set_runtime_config(
        &self,
        enable_notification: bool,
        enable_sensory: bool,
    ) -> DbResult<RuntimeConfigRecord> {
        let updated_at = now_iso8601();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO runtime_config (id, enable_notification, enable_sensory, updated_at) \
             VALUES (1, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
               enable_notification = excluded.enable_notification, \
               enable_sensory = excluded.enable_sensory, \
               updated_at = excluded.updated_at",
            params![
                if enable_notification { 1 } else { 0 },
                if enable_sensory { 1 } else { 0 },
                updated_at.clone()
            ],
        )
        .await?;
        Ok(RuntimeConfigRecord {
            enable_notification,
            enable_sensory,
            updated_at,
        })
    }

    pub async fn add_notification_token(&self, user_id: &str, token: &str) -> DbResult<()> {
        let created_at = now_iso8601();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO notification_tokens (user_id, token, created_at) VALUES (?, ?, ?) \
             ON CONFLICT(user_id, token) DO NOTHING",
            params![user_id, token, created_at],
        )
        .await?;
        Ok(())
    }

    pub async fn remove_notification_token(&self, user_id: &str, token: &str) -> DbResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM notification_tokens WHERE user_id = ? AND token = ?",
            params![user_id, token],
        )
        .await?;
        Ok(())
    }

    pub async fn list_notification_tokens(&self, user_id: &str) -> DbResult<Vec<String>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT token FROM notification_tokens WHERE user_id = ? ORDER BY created_at DESC",
                params![user_id],
            )
            .await?;
        let mut tokens = Vec::new();
        while let Some(row) = rows.next().await? {
            let token: String = row.get(0)?;
            tokens.push(token);
        }
        Ok(tokens)
    }

    pub async fn create_admin_session(
        &self,
        session_id: &str,
        created_at: &str,
        last_seen_at: &str,
        password_fingerprint: &str,
    ) -> DbResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO admin_sessions (session_id, created_at, last_seen_at, password_fingerprint) \
             VALUES (?, ?, ?, ?)",
            params![session_id, created_at, last_seen_at, password_fingerprint],
        )
        .await?;
        Ok(())
    }

    pub async fn get_admin_session(
        &self,
        session_id: &str,
    ) -> DbResult<Option<AdminSessionRecord>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT session_id, created_at, last_seen_at, password_fingerprint \
                 FROM admin_sessions WHERE session_id = ? LIMIT 1",
                params![session_id],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let session_id: String = row.get(0)?;
            let created_at: String = row.get(1)?;
            let last_seen_at: String = row.get(2)?;
            let password_fingerprint: String = row.get(3)?;
            return Ok(Some(AdminSessionRecord {
                session_id,
                created_at,
                last_seen_at,
                password_fingerprint,
            }));
        }
        Ok(None)
    }

    pub async fn update_admin_session_last_seen(
        &self,
        session_id: &str,
        last_seen_at: &str,
    ) -> DbResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE admin_sessions SET last_seen_at = ? WHERE session_id = ?",
            params![last_seen_at, session_id],
        )
        .await?;
        Ok(())
    }

    pub async fn delete_admin_session(&self, session_id: &str) -> DbResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM admin_sessions WHERE session_id = ?",
            params![session_id],
        )
        .await?;
        Ok(())
    }

    pub async fn delete_admin_sessions_not_matching_fingerprint(
        &self,
        password_fingerprint: &str,
    ) -> DbResult<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "DELETE FROM admin_sessions WHERE password_fingerprint != ?",
            params![password_fingerprint],
        )
        .await?;
        Ok(())
    }
}
