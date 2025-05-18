use std::hash::Hash;

use cron::Schedule;
use serde::{Deserialize, Serialize};

#[derive(Eq, PartialEq, Clone, Serialize, Deserialize)]
pub struct ScheduleRecord {
    pub schedule: Schedule,
    pub message: String,
}

impl Hash for ScheduleRecord {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.schedule.source().hash(state);
        self.message.hash(state);
    }
}
