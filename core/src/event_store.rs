use crate::db::{Db, DbResult};
use crate::event::Event;
use std::sync::Arc;

#[derive(Clone)]
pub struct EventStore {
    db: Arc<Db>,
}

impl EventStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    pub async fn append(&self, event: &Event) -> DbResult<()> {
        self.db.insert_event(event).await
    }

    pub async fn latest(&self, limit: usize) -> DbResult<Vec<Event>> {
        self.db.load_latest_events(limit).await
    }

    pub async fn get_by_id(&self, event_id: &str) -> DbResult<Option<Event>> {
        self.db.get_event_by_id(event_id).await
    }

    pub async fn list(
        &self,
        limit: usize,
        before_ts: Option<&str>,
        desc: bool,
    ) -> DbResult<Vec<Event>> {
        self.db.load_events(limit, before_ts, desc).await
    }

    pub async fn list_before_anchor(
        &self,
        anchor_ts: &str,
        anchor_event_id: &str,
        limit: usize,
    ) -> DbResult<Vec<Event>> {
        self.db
            .load_events_before_anchor(anchor_ts, anchor_event_id, limit)
            .await
    }

    pub async fn list_after_anchor(
        &self,
        anchor_ts: &str,
        anchor_event_id: &str,
        limit: usize,
    ) -> DbResult<Vec<Event>> {
        self.db
            .load_events_after_anchor(anchor_ts, anchor_event_id, limit)
            .await
    }
}
