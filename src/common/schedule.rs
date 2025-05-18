use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct ScheduleRecord {
    pub expression: String,
    pub message: String,
}
