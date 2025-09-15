use chrono::{DateTime, NaiveDateTime, NaiveTime};
use chrono_tz::Tz;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{
        Annotated, CallToolResult, Content, Implementation, ListResourcesResult, RawResource,
        ReadResourceResult, ResourceContents, ResourceUpdatedNotificationParam, ServerCapabilities,
        ServerInfo, SubscribeRequestParam, UnsubscribeRequestParam,
    },
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router,
};
use rmcp::{Peer, RoleServer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::{fs, sync::Mutex, time};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetScheduleRequest {
    #[schemars(description = "Unique name for the schedule")]
    pub name: String,
    #[schemars(description = "Time for the schedule\n\
        - For \"once\" cycle: local time (e.g., \"2024-01-15T14:30:00\") or ISO 8601 format (e.g., \"2024-01-15T14:30:00+09:00\")\n\
        - For \"daily\" cycle: HH:MM format (e.g., \"14:30\") or HH:MM:SS format (e.g., \"14:30:00\")")]
    pub time: String,
    #[schemars(description = "Cycle for the schedule (\"daily\" or \"once\")")]
    pub cycle: String,
    #[schemars(description = "Message (title of notification)")]
    pub message: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveScheduleRequest {
    #[schemars(description = "Name of the schedule to remove")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    pub time: NaiveDateTime, // For one-time schedules, date is ignored
    pub cycle: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiredSchedule {
    pub name: String,
    pub scheduled_time: NaiveDateTime,
    pub fired_time: NaiveDateTime,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SchedulerService {
    tool_router: ToolRouter<Self>,
    data_dir: String,
    schedules: Arc<Mutex<HashMap<String, Schedule>>>,
    fired_schedules: Arc<Mutex<Vec<FiredSchedule>>>,
    timezone: chrono_tz::Tz,
    subscriptions: Arc<Mutex<HashMap<String, Peer<RoleServer>>>>,
}

impl SchedulerService {
    pub fn new(data_dir: String) -> Result<Self, ErrorData> {
        if data_dir.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "DATA_DIR environment variable is required",
                Some(json!({"reason": "DATA_DIR environment variable not set"})),
            ));
        }

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
            fired_schedules: Arc::new(Mutex::new(Vec::new())),
            timezone,
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
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

            let loaded_schedules: HashMap<String, Schedule> = serde_json::from_str(&content)
                .map_err(|e| {
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

    async fn add_fired_schedule(&self, fired_schedule: FiredSchedule) -> Result<(), ErrorData> {
        let mut fired_schedules = self.fired_schedules.lock().await;
        fired_schedules.push(fired_schedule);
        // Keep only the most recent 1000 entries
        const MAX_FIRED_SCHEDULES: usize = 1000;
        if fired_schedules.len() > MAX_FIRED_SCHEDULES {
            let excess = fired_schedules.len() - MAX_FIRED_SCHEDULES;
            fired_schedules.drain(0..excess);
        }
        Ok(())
    }

    pub async fn start_scheduler_daemon(self: Arc<Self>) -> Result<(), ErrorData> {
        // Load existing schedules and fired schedules on startup
        self.load_schedules().await?;

        // Allow loop interval to be configured via env var (milliseconds)
        let interval_ms = std::env::var("SCHEDULER_LOOP_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60_000); // default: 60s
        let mut interval = time::interval(Duration::from_millis(interval_ms));

        loop {
            interval.tick().await;
            if let Err(e) = self.check_and_fire_schedules().await {
                eprintln!("Error in scheduler daemon: {:?}", e);
            }
        }
    }

    async fn check_and_fire_schedules(&self) -> Result<(), ErrorData> {
        let now = chrono::Utc::now()
            .with_timezone(&self.timezone)
            .naive_local();

        let mut schedules_to_remove = Vec::new();

        // Check schedules that need to be fired
        {
            let schedules = self.schedules.lock().await;

            for (name, schedule) in schedules.iter() {
                let should_fire = match schedule.cycle.as_str() {
                    "daily" => {
                        // For daily schedules, fire when current time matches scheduled time
                        let delta = now.time() - schedule.time.time();
                        delta.num_minutes() == 0
                    }
                    "once" => {
                        // For one-time schedules, fire if current time is past scheduled time
                        now >= schedule.time
                    }
                    _ => false,
                };

                if should_fire {
                    let fired_schedule = FiredSchedule {
                        name: schedule.name.clone(),
                        scheduled_time: schedule.time,
                        fired_time: now,
                        message: schedule.message.clone(),
                    };

                    // Add to fired schedules
                    if let Err(e) = self.add_fired_schedule(fired_schedule).await {
                        eprintln!("Failed to add fired schedule: {:?}", e);
                        continue;
                    }

                    eprintln!(
                        "🔔 Schedule fired: {} - {}",
                        schedule.name, schedule.message
                    );

                    // Send notification to subscribers
                    self.notify_fired_schedule(&schedule.message).await;

                    // Mark one-time schedules for removal
                    if schedule.cycle == "once" {
                        schedules_to_remove.push(name.clone());
                    }
                }
            }
        }

        // Remove one-time schedules that have been fired
        if !schedules_to_remove.is_empty() {
            let mut schedules = self.schedules.lock().await;
            for name in schedules_to_remove {
                schedules.remove(&name);
            }
            drop(schedules);
            self.save_schedules().await?;
        }

        Ok(())
    }

    async fn notify_fired_schedule(&self, title: &str) {
        let subscriptions = self.subscriptions.lock().await;

        for (uri, peer) in subscriptions.iter() {
            if uri.starts_with("fired_schedule://") {
                let params = ResourceUpdatedNotificationParam {
                    uri: uri.clone(),
                    title: title.to_string(),
                };

                if let Err(e) = peer.notify_resource_updated(params).await {
                    eprintln!("Failed to send resource update notification: {:?}", e);
                }
            }
        }
    }

    fn validate_cycle(&self, cycle: &str) -> Result<(), ErrorData> {
        match cycle {
            "daily" | "once" => Ok(()),
            _ => Err(ErrorData::invalid_params(
                "Error: cycle: invalid value",
                Some(json!({"cycle": cycle})),
            )),
        }
    }

    fn parse_time(&self, time: &str, cycle: &str) -> Result<DateTime<Tz>, ErrorData> {
        match cycle {
            "once" => {
                // For one-time schedules, expect ISO 8601 format (or without timezone)
                if let Ok(naive_datetime) =
                    chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M:%S")
                {
                    naive_datetime.and_local_timezone(self.timezone).earliest().ok_or_else(|| {
                        ErrorData::invalid_params(
                            "Error: time: invalid datetime",
                            Some(json!({"time": time, "expected": "ISO 8601 format for one-time schedules"})),
                        )
                    })
                } else if let Ok(naive_time) = NaiveTime::parse_from_str(time, "%H:%M:%S") {
                    // If only time is provided, assume today in local timezone
                    let today_datetime = chrono::Utc::now()
                        .date_naive()
                        .and_time(naive_time)
                        .and_local_timezone(self.timezone)
                        .earliest().ok_or_else(|| {
                            ErrorData::invalid_params(
                                "Error: time: invalid datetime",
                                Some(json!({"time": time, "expected": "ISO 8601 format for one-time schedules"})),
                            )
                        })?;

                    // Check if the time has already passed today
                    let now = chrono::Utc::now().with_timezone(&self.timezone);
                    if today_datetime <= now {
                        return Err(ErrorData::invalid_params(
                            "Error: time: past time not allowed for one-time schedules",
                            Some(json!({"time": time, "reason": "specified time has already passed today"})),
                        ));
                    }

                    Ok(today_datetime)
                } else if let Ok(naive_time) = NaiveTime::parse_from_str(time, "%H:%M") {
                    // If only time is provided without seconds, assume today in local timezone
                    let today_datetime = chrono::Utc::now()
                        .date_naive()
                        .and_time(naive_time)
                        .and_local_timezone(self.timezone)
                        .earliest().ok_or_else(|| {
                            ErrorData::invalid_params(
                                "Error: time: invalid datetime",
                                Some(json!({"time": time, "expected": "ISO 8601 format for one-time schedules"})),
                            )
                        })?;

                    // Check if the time has already passed today
                    let now = chrono::Utc::now().with_timezone(&self.timezone);
                    if today_datetime <= now {
                        return Err(ErrorData::invalid_params(
                            "Error: time: past time not allowed for one-time schedules",
                            Some(json!({"time": time, "reason": "specified time has already passed today"})),
                        ));
                    }

                    Ok(today_datetime)
                } else {
                    Ok(DateTime::parse_from_rfc3339(time).map_err(|_| {
                        ErrorData::invalid_params(
                            "Error: time: invalid format",
                            Some(
                                json!({"time": time, "expected": "ISO 8601 format for one-time schedules"}),
                            ),
                        )
                    })?.with_timezone(&self.timezone))
                }
            }
            "daily" => {
                // For daily schedules, expect HH:MM format or HH:MM:SS
                if let Ok(naive_time) = NaiveTime::parse_from_str(time, "%H:%M:%S") {
                    // Construct a DateTime<Utc> for today with the given time
                    chrono::Utc::now()
                        .date_naive()
                        .and_time(naive_time)
                        .and_local_timezone(self.timezone)
                        .earliest().ok_or_else(|| {
                            ErrorData::invalid_params(
                                "Error: time: invalid format",
                                Some(json!({"time": time, "expected": "HH:MM:SS format for daily schedules"})),
                            )
                        })
                } else if let Ok(naive_time) = NaiveTime::parse_from_str(time, "%H:%M") {
                    // If seconds are not provided, assume 00 seconds
                    chrono::Utc::now()
                        .date_naive()
                        .and_time(naive_time)
                        .and_local_timezone(self.timezone)
                        .earliest().ok_or_else(|| {
                            ErrorData::invalid_params(
                                "Error: time: invalid format",
                                Some(json!({"time": time, "expected": "HH:MM format for daily schedules"})),
                            )
                        })
                } else {
                    Err(ErrorData::invalid_params(
                        "Error: time: invalid format",
                        Some(
                            json!({"time": time, "expected": "HH:MM or HH:MM:SS format for daily schedules"}),
                        ),
                    ))
                }
            }
            _ => unreachable!(), // Already validated in validate_cycle
        }
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
        let time = self
            .parse_time(&request.time, &request.cycle)?
            .naive_local();

        // Load existing schedules
        self.load_schedules().await?;

        let schedule = Schedule {
            name: request.name.clone(),
            time,
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
            meta: None,
        })
    }

    #[tool(description = "Retrieves all currently active schedules")]
    pub async fn get_schedules(&self) -> Result<CallToolResult, ErrorData> {
        self.load_schedules().await?;

        let schedules = self.schedules.lock().await;
        let schedules_list: Vec<&Schedule> = schedules.values().collect();

        let content = serde_json::to_string(&schedules_list).map_err(|e| {
            ErrorData::internal_error(
                "Failed to serialize schedules",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        Ok(CallToolResult {
            content: vec![Content::text(content)],
            structured_content: None,
            is_error: Some(false),
            meta: None,
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
            meta: None,
        })
    }
}

#[tool_handler]
impl ServerHandler for SchedulerService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Scheduler MCP server for managing time-based message notifications".into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_resources_subscribe()
                .build(),
            server_info: Implementation {
                name: env!("CARGO_CRATE_NAME").to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            ..Default::default()
        }
    }

    async fn list_resources(
        &self,
        _param: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let resources = vec![Annotated::new(
            RawResource {
                uri: "fired_schedule://recent".to_string(),
                name: "Recent Fired Schedules".to_string(),
                description: Some("Most recent 100 fired schedule notifications".to_string()),
                mime_type: Some("application/json".to_string()),
                size: None,
            },
            None,
        )];

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        param: rmcp::model::ReadResourceRequestParam,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let fired_schedules = self.fired_schedules.lock().await;
        let uri = param.uri.as_str();

        if uri != "fired_schedule://recent" {
            return Err(ErrorData::invalid_params(
                "Unknown resource URI",
                Some(json!({"uri": uri})),
            ));
        }

        let recent_count = std::cmp::min(100, fired_schedules.len());
        let recent_schedules = if fired_schedules.len() > recent_count {
            &fired_schedules[fired_schedules.len() - recent_count..]
        } else {
            &fired_schedules[..]
        };

        let data = serde_json::to_string_pretty(recent_schedules).map_err(|e| {
            ErrorData::internal_error(
                "Failed to serialize recent fired schedules",
                Some(json!({"reason": e.to_string()})),
            )
        })?;

        Ok(ReadResourceResult {
            contents: vec![ResourceContents::TextResourceContents {
                uri: param.uri,
                mime_type: Some("application/json".to_string()),
                text: data,
                meta: None,
            }],
        })
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParam,
        context: RequestContext<rmcp::RoleServer>,
    ) -> Result<(), ErrorData> {
        {
            let mut subscriptions = self.subscriptions.lock().await;
            subscriptions.insert(request.uri.clone(), context.peer);
        }

        eprintln!("Subscribed to resource: {}", request.uri);
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParam,
        _context: RequestContext<rmcp::RoleServer>,
    ) -> Result<(), ErrorData> {
        let mut subscriptions = self.subscriptions.lock().await;
        subscriptions.remove(&request.uri);

        eprintln!("Unsubscribed from resource: {}", request.uri);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;
    use tokio;

    use tempfile;

    // Helper function to create test service with auto-cleanup temporary directory
    async fn create_test_service() -> SchedulerService {
        unsafe {
            std::env::set_var("TZ", "Asia/Tokyo");
        }
        let temp_dir = tempfile::tempdir().unwrap();
        let test_dir = temp_dir.path().to_string_lossy().to_string();
        SchedulerService::new(test_dir).unwrap()
    }

    #[tokio::test]
    async fn test_set_schedule_daily() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "test_daily".to_string(),
            time: "09:30".to_string(),
            cycle: "daily".to_string(),
            message: "Daily reminder".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.content.is_empty());
        // Verify successful response content
    }

    #[tokio::test]
    async fn test_set_schedule_one_time() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "test_once".to_string(),
            time: "2024-12-25T10:00:00+09:00".to_string(),
            cycle: "once".to_string(),
            message: "Christmas reminder".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_schedule_local_time() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "test_once".to_string(),
            time: "2024-12-25T10:00:00".to_string(),
            cycle: "once".to_string(),
            message: "Christmas reminder".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_ok());

        let result2 = service.get_schedules().await;
        assert!(result2.is_ok());

        let response = result2.unwrap();
        assert_eq!(response.content.len(), 1);
        assert_eq!(
            response.content[0].as_text().unwrap().text,
            "[{\"name\":\"test_once\",\"time\":\"2024-12-25T10:00:00\",\"cycle\":\"once\",\"message\":\"Christmas reminder\"}]",
        );
    }

    #[tokio::test]
    async fn test_set_schedule_once_time_without_date() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "test_once_time_without_date".to_string(),
            time: "23:59:00".to_string(),
            cycle: "once".to_string(),
            message: "Today's one-time schedule".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_schedule_once_time_without_date_without_seconds() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "test_once_time_without_date_without_seconds".to_string(),
            time: "23:59".to_string(),
            cycle: "once".to_string(),
            message: "Today's one-time schedule without seconds".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_schedule_invalid_cycle() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "test_invalid".to_string(),
            time: "09:30".to_string(),
            cycle: "weekly".to_string(),
            message: "Invalid cycle".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_schedules_empty() {
        let service = create_test_service().await;
        let result = service.get_schedules().await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.content.is_empty());
        // Returns empty array when no schedules exist
    }

    #[tokio::test]
    async fn test_get_schedules_with_data() {
        let service = create_test_service().await;

        // Create a schedule first
        let request = SetScheduleRequest {
            name: "test_schedule".to_string(),
            time: "15:00".to_string(),
            cycle: "daily".to_string(),
            message: "Test message".to_string(),
        };
        service.set_schedule(Parameters(request)).await.unwrap();

        let result = service.get_schedules().await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.content.is_empty());
        // Returns schedule data after creation
    }

    #[tokio::test]
    async fn test_remove_schedule_existing() {
        let service = create_test_service().await;

        // Create a schedule first
        let set_request = SetScheduleRequest {
            name: "to_remove".to_string(),
            time: "12:00".to_string(),
            cycle: "daily".to_string(),
            message: "Will be removed".to_string(),
        };
        service.set_schedule(Parameters(set_request)).await.unwrap();

        // Remove the schedule
        let remove_request = RemoveScheduleRequest {
            name: "to_remove".to_string(),
        };
        let result = service.remove_schedule(Parameters(remove_request)).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.content.is_empty());
        // Confirms successful removal
    }

    #[tokio::test]
    async fn test_remove_schedule_nonexistent() {
        let service = create_test_service().await;
        let request = RemoveScheduleRequest {
            name: "nonexistent".to_string(),
        };

        let result = service.remove_schedule(Parameters(request)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_schedule_empty_name() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "".to_string(),
            time: "09:30".to_string(),
            cycle: "daily".to_string(),
            message: "Test message".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_schedule_empty_message() {
        let service = create_test_service().await;
        let request = SetScheduleRequest {
            name: "test".to_string(),
            time: "09:30".to_string(),
            cycle: "daily".to_string(),
            message: "".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_schedule_past_time() {
        let service = create_test_service().await;

        // Use a time that's definitely in the past (early morning)
        let request = SetScheduleRequest {
            name: "test_past".to_string(),
            time: "00:00:00".to_string(),
            cycle: "once".to_string(),
            message: "Past time test".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_err());

        // Verify error message
        if let Err(error) = result {
            assert!(error.message.contains("past time not allowed"));
        }
    }

    #[tokio::test]
    async fn test_set_schedule_past_time_without_seconds() {
        let service = create_test_service().await;

        // Use a time that's definitely in the past (early morning without seconds)
        let request = SetScheduleRequest {
            name: "test_past_no_seconds".to_string(),
            time: "00:00".to_string(),
            cycle: "once".to_string(),
            message: "Past time test without seconds".to_string(),
        };

        let result = service.set_schedule(Parameters(request)).await;
        assert!(result.is_err());

        // Verify error message
        if let Err(error) = result {
            assert!(error.message.contains("past time not allowed"));
        }
    }
}
