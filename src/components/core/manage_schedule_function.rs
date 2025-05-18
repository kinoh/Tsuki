use crate::adapter::openai::Function;
use crate::common::broadcast::IdentifiedBroadcast;
use crate::common::events::Event;
use crate::common::repository::Repository;
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;

#[derive(Deserialize)]
pub struct ManageScheduleFunctionArguments {
    pub operation: String,
    pub expression: Option<String>,
    pub message: Option<String>,
}

pub struct ManageScheduleFunction {
    pub repository: Arc<RwLock<Repository>>,
    pub broadcast: IdentifiedBroadcast<Event>,
}

#[async_trait]
impl Function for ManageScheduleFunction {
    fn name(&self) -> &'static str {
        "manage_schedule"
    }

    fn description(&self) -> &'static str {
        "Manage schedules that send system message (Text modality message sent by system user) at specified times using cron expression"
    }

    fn args_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "\"add\" or \"remove\" or \"list\""
                },
                "expression": {
                    "type": "string",
                    "description": "cron expression to specify times to send; required for \"add\" or \"remove\"; empty for \"list\""
                },
                "message": {
                    "type": "string",
                    "description": "message sent by system user; required for \"add\" or \"remove\"; empty for \"list\""
                }
            },
            "required": ["operation", "expression", "message"],
            "additionalProperties": false
        })
    }

    async fn call(&self, args_json: &str) -> Result<String, String> {
        let args: ManageScheduleFunctionArguments =
            serde_json::from_str(&args_json).map_err(|_| "invalid arguments".to_string())?;
        match args.operation.as_str() {
            "add" => {
                if let (Some(expression), Some(message)) = (args.expression, args.message) {
                    self.repository
                        .write()
                        .await
                        .append_schedule(expression, message)
                        .map_err(|e| e.to_string())?;
                    if let Err(e) = self.broadcast.send(Event::SchedulesUpdated) {
                        error!("send error in function call: {}", e);
                    };
                    Ok(String::from("success"))
                } else {
                    Err(String::from("expression and message required for \"add\""))
                }
            }
            "remove" => {
                if let (Some(expression), Some(message)) = (args.expression, args.message) {
                    self.repository
                        .write()
                        .await
                        .remove_schedule(expression, message)
                        .map_err(|e| e.to_string())?;
                    if let Err(e) = self.broadcast.send(Event::SchedulesUpdated) {
                        error!("send error in function call: {}", e);
                    };
                    Ok(String::from("success"))
                } else {
                    Err(String::from(
                        "expression and message required for \"remove\"",
                    ))
                }
            }
            "list" => Ok(self
                .repository
                .read()
                .await
                .schedules()
                .iter()
                .map(|s| {
                    format!(
                        "<schedule><expression>{}</expression><message>{}</message></schedule>",
                        s.schedule.source(),
                        s.message
                    )
                })
                .collect::<Vec<String>>()
                .concat()),
            _ => Err(String::from("unexpected operation")),
        }
    }
}
