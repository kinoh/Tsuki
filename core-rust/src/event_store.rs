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
}
