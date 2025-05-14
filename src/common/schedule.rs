use serde::{Deserialize, Serialize};

use super::events::Event;

#[derive(Serialize, Deserialize)]
pub struct ScheduleRecord {
    pub expression: String,
    pub event: Event,
}
