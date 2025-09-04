use chrono::{DateTime, NaiveTime};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router,
};
use std::future::Future;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::sync::Arc;
use tokio::{fs, sync::Mutex};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetScheduleRequest {
    pub name: String,
    pub time: String,
    pub cycle: String, // "daily" | "none"
    pub message: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveScheduleRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    pub time: String,
    pub cycle: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiredSchedule {
    pub name: String,
    pub scheduled_time: String,
    pub fired_time: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SchedulerService {
    tool_router: ToolRouter<Self>,
    data_dir: String,
    schedules: Arc<Mutex<HashMap<String, Schedule>>>,
    timezone: chrono_tz::Tz,
}

impl SchedulerService {
    pub fn new(data_dir: String) -> Result<Self, ErrorData> {
        let timezone_str = env::var("TZ").map_err(|_| {
            ErrorData::invalid_params(
                "TZ environment variable is required",
                Some(json!({"reason": "TZ environment variable not set"})),
            )
        })?;

        let timezone = timezone_str.parse::<chrono_tz::Tz>().map_err(|e| {
            ErrorData::invalid_params(
                "Invalid timezone in TZ environment variable",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        Ok(Self {
            tool_router: Self::tool_router(),
            data_dir,
            schedules: Arc::new(Mutex::new(HashMap::new())),
            timezone,
        })
    }

    async fn ensure_data_dir(&self) -> Result<(), ErrorData> {
        if !Path::new(&self.data_dir).exists() {
            fs::create_dir_all(&self.data_dir).await.map_err(|e| {
                ErrorData::internal_error(
                    "Failed to create data directory",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;
        }
        Ok(())
    }

    async fn schedules_file_path(&self) -> String {
        format!("{}/schedules.json", self.data_dir)
    }

    async fn load_schedules(&self) -> Result<(), ErrorData> {
        self.ensure_data_dir().await?;
        
        let file_path = self.schedules_file_path().await;
        if Path::new(&file_path).exists() {
            let content = fs::read_to_string(&file_path).await.map_err(|e| {
                ErrorData::internal_error(
                    "Failed to read schedules file",
                    Some(json!({"reason": e.to_string()})),
                )
            })?;

            let loaded_schedules: HashMap<String, Schedule> = 
                serde_json::from_str(&content).map_err(|e| {
                    ErrorData::internal_error(
                        "Failed to parse schedules file",
                        Some(json!({"reason": e.to_string()})),
                    )
                })?;

            let mut schedules = self.schedules.lock().await;
            *schedules = loaded_schedules;
        }
        Ok(())
    }

    async fn save_schedules(&self) -> Result<(), ErrorData> {
        self.ensure_data_dir().await?;

        let schedules = self.schedules.lock().await;
        let content = serde_json::to_string_pretty(&*schedules).map_err(|e| {
            ErrorData::internal_error(
                "Failed to serialize schedules",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        let file_path = self.schedules_file_path().await;
        fs::write(&file_path, content).await.map_err(|e| {
            ErrorData::internal_error(
                "Failed to write schedules file",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        Ok(())
    }

    fn validate_cycle(&self, cycle: &str) -> Result<(), ErrorData> {
        match cycle {
            "daily" | "none" => Ok(()),
            _ => Err(ErrorData::invalid_params(
                "Error: cycle: invalid value",
                Some(json!({"cycle": cycle})),
            )),
        }
    }

    fn validate_time_format(&self, time: &str, cycle: &str) -> Result<(), ErrorData> {
        match cycle {
            "none" => {
                // For one-time schedules, expect ISO 8601 format
                DateTime::parse_from_rfc3339(time).map_err(|_| {
                    ErrorData::invalid_params(
                        "Error: time: invalid format",
                        Some(json!({"time": time, "expected": "ISO 8601 format for one-time schedules"})),
                    )
                })?;
            }
            "daily" => {
                // For daily schedules, expect HH:MM format
                NaiveTime::parse_from_str(time, "%H:%M").map_err(|_| {
                    ErrorData::invalid_params(
                        "Error: time: invalid format", 
                        Some(json!({"time": time, "expected": "HH:MM format for daily schedules"})),
                    )
                })?;
            }
            _ => unreachable!(), // Already validated in validate_cycle
        }
        Ok(())
    }
}

#[tool_router]
impl SchedulerService {
    #[tool(description = "Creates or updates a scheduled notification")]
    pub async fn set_schedule(
        &self,
        params: Parameters<SetScheduleRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;

        if request.name.is_empty() {
            return Err(ErrorData::invalid_params(
                "Error: name: required",
                Some(json!({"name": request.name})),
            ));
        }

        if request.message.is_empty() {
            return Err(ErrorData::invalid_params(
                "Error: message: required",
                Some(json!({"message": request.message})),
            ));
        }

        self.validate_cycle(&request.cycle)?;
        self.validate_time_format(&request.time, &request.cycle)?;

        // Load existing schedules
        self.load_schedules().await?;

        let schedule = Schedule {
            name: request.name.clone(),
            time: request.time,
            cycle: request.cycle,
            message: request.message,
        };

        {
            let mut schedules = self.schedules.lock().await;
            schedules.insert(request.name, schedule);
        }

        self.save_schedules().await?;

        Ok(CallToolResult {
            content: vec![Content::text("Succeeded".to_string())],
            structured_content: None,
            is_error: Some(false),
        })
    }

    #[tool(description = "Retrieves all currently active schedules")]
    pub async fn get_schedules(&self) -> Result<CallToolResult, ErrorData> {
        self.load_schedules().await?;

        let schedules = self.schedules.lock().await;
        let schedules_list: Vec<&Schedule> = schedules.values().collect();
        
        let content = serde_json::to_string_pretty(&schedules_list).map_err(|e| {
            ErrorData::internal_error(
                "Failed to serialize schedules",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        Ok(CallToolResult {
            content: vec![Content::text(content)],
            structured_content: None,
            is_error: Some(false),
        })
    }

    #[tool(description = "Removes a scheduled notification")]
    pub async fn remove_schedule(
        &self,
        params: Parameters<RemoveScheduleRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;

        if request.name.is_empty() {
            return Err(ErrorData::invalid_params(
                "Error: name: required",
                Some(json!({"name": request.name})),
            ));
        }

        self.load_schedules().await?;

        {
            let mut schedules = self.schedules.lock().await;
            if !schedules.contains_key(&request.name) {
                return Err(ErrorData::invalid_params(
                    "Error: name: not found",
                    Some(json!({"name": request.name})),
                ));
            }
            schedules.remove(&request.name);
        }

        self.save_schedules().await?;

        Ok(CallToolResult {
            content: vec![Content::text("Succeeded".to_string())],
            structured_content: None,
            is_error: Some(false),
        })
    }
}

#[tool_handler]
impl ServerHandler for SchedulerService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Scheduler MCP server for managing time-based message notifications".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation { 
                name: env!("CARGO_CRATE_NAME").to_owned(), 
                version: env!("CARGO_PKG_VERSION").to_owned() 
            },
            ..Default::default()
        }
    }
}