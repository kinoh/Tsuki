use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::warn;
use uuid::Uuid;
use weaviate_community::collections::objects::Object;
use weaviate_community::collections::query::GetQuery;
use weaviate_community::collections::schema::{Class, Properties, Property};
use weaviate_community::WeaviateClient;

use super::Repository;
use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, SessionId};
use crate::common::schedule::ScheduleRecord;

const CLASS_SESSION: &str = "Session";
const CLASS_MESSAGE: &str = "Message";
const CLASS_MEMORY: &str = "Memory";
const CLASS_SCHEDULE: &str = "Schedule";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
}

impl ToString for Role {
    fn to_string(&self) -> String {
        match self {
            Role::User => "user".to_string(),
            Role::Assistant => "assistant".to_string(),
        }
    }
}

impl std::str::FromStr for Role {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "user" => Ok(Role::User),
            "assistant" => Ok(Role::Assistant),
            _ => anyhow::bail!("Invalid role: {}", s),
        }
    }
}

pub struct WeaviateRepository {
    client: WeaviateClient,
}

impl WeaviateRepository {
    pub async fn new(url: &str) -> Result<Self> {
        let client = WeaviateClient::builder(url).build()?;
        let schema = client.schema.get().await.map_err(Into::<Error>::into)?;
        if !schema
            .classes
            .iter()
            .any(|class| class.class == CLASS_SESSION)
        {
            let session_class = Class::builder(CLASS_SESSION)
                .with_description("A session of conversation")
                .with_properties(Properties::new(vec![Property::builder(
                    "createdAt",
                    vec!["date"],
                )
                .with_description("The date and time the session was created")
                .build()]))
                .build();
            client.schema.create_class(&session_class).await?;
        }
        if !schema
            .classes
            .iter()
            .any(|class| class.class == CLASS_MESSAGE)
        {
            let message_class = Class::builder(CLASS_MESSAGE)
                .with_description("A message in a session")
                .with_properties(Properties::new(vec![
                    Property::builder("role", vec!["string"])
                        .with_description("The role of the message")
                        .build(),
                    Property::builder("content", vec!["text"])
                        .with_description("The content of the message")
                        .build(),
                    Property::builder("timestamp", vec!["number"])
                        .with_description("The timestamp of the message")
                        .build(),
                    Property::builder("responseId", vec!["string"])
                        .with_description("The response ID of the message")
                        .build(),
                    Property::builder("session", vec![CLASS_SESSION])
                        .with_description("The session this message belongs to")
                        .build(),
                ]))
                .build();
            client.schema.create_class(&message_class).await?;
        }
        if !schema
            .classes
            .iter()
            .any(|class| class.class == CLASS_MEMORY)
        {
            let memory_class = Class::builder(CLASS_MEMORY)
                .with_description("A memory")
                .with_properties(Properties::new(vec![
                    Property::builder("content", vec!["text"])
                        .with_description("The content of the memory")
                        .build(),
                    Property::builder("timestamp", vec!["number"])
                        .with_description("The timestamp of the memory")
                        .build(),
                ]))
                .build();
            client.schema.create_class(&memory_class).await?;
        }
        if !schema
            .classes
            .iter()
            .any(|class| class.class == CLASS_SCHEDULE)
        {
            let schedule_class = Class::builder(CLASS_SCHEDULE)
                .with_description("A schedule")
                .with_properties(Properties::new(vec![
                    Property::builder("expression", vec!["string"])
                        .with_description("The cron expression for the schedule")
                        .build(),
                    Property::builder("message", vec!["string"])
                        .with_description("The message to be sent on schedule")
                        .build(),
                ]))
                .build();
            client.schema.create_class(&schedule_class).await?;
        }

        Ok(Self {
            client,
        })
    }
}

#[async_trait]
impl Repository for WeaviateRepository {
    async fn get_or_create_session(&self) -> Result<SessionId> {
        // Check if there's already a session by querying the latest one
        let query = GetQuery::builder(CLASS_SESSION, vec!["_additional { id }"])
            .with_sort("createdAt:desc")
            .with_limit(1)
            .build();
        
        let result = self
            .client
            .query
            .get(query)
            .await
            .context("Failed to query sessions")?;

        // If a session exists, return it
        if let Some(sessions) = extract_objects(&result, CLASS_SESSION) {
            if let Some(session) = sessions.first() {
                if let Some(id) = extract_additional_id(session) {
                    return Ok(id.to_string());
                }
            }
        }

        // Create a new session if none exists
        let new_object = Object::builder(
            CLASS_SESSION,
            json!({
                "createdAt": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .build();
        let new_session = self
            .client
            .objects
            .create(&new_object, None)
            .await
            .context("Failed to create session")?;

        let new_id = new_session.id.unwrap().to_string();
        Ok(new_id)
    }

    async fn has_session(&self) -> bool {
        let query = GetQuery::builder(CLASS_SESSION, vec!["_additional { id }"])
            .with_limit(1)
            .build();
        
        if let Ok(result) = self.client.query.get(query).await {
            if let Some(sessions) = extract_objects(&result, CLASS_SESSION) {
                return !sessions.is_empty();
            }
        }
        false
    }

    async fn clear_session(&self) -> Result<()> {
        // Get the latest session
        let query = GetQuery::builder(CLASS_SESSION, vec!["_additional { id }"])
            .with_sort("createdAt:desc")
            .with_limit(1)
            .build();
        
        let result = self
            .client
            .query
            .get(query)
            .await
            .context("Failed to query sessions")?;

        if let Some(sessions) = extract_objects(&result, CLASS_SESSION) {
            if let Some(session) = sessions.first() {
                if let Some(id) = extract_additional_id(session) {
                    // Delete messages of the session
                    let query = GetQuery::builder(CLASS_MESSAGE, vec!["_additional { id }"])
                        .with_where(
                            &json!({
                                "path": ["session"],
                                "operator": "Equal",
                                "valueString": id,
                            })
                            .to_string(),
                        )
                        .build();
                    let messages = self
                        .client
                        .query
                        .get(query)
                        .await
                        .context("Failed to get messages of session")?;
                    if let Some(messages) = extract_objects(&messages, CLASS_MESSAGE) {
                        for message in messages {
                            if let Some(msg_id) = extract_additional_id(message) {
                                self.client
                                    .objects
                                    .delete(CLASS_MESSAGE, &msg_id, None, None)
                                    .await
                                    .context("Failed to delete message")?;
                            }
                        }
                    }

                    let uuid = Uuid::parse_str(&id.to_string()).context("Invalid UUID format")?;

                    // Delete the session
                    self.client
                        .objects
                        .delete(CLASS_SESSION, &uuid, None, None)
                        .await
                        .context("Failed to delete session")?;
                }
            }
        }
        Ok(())
    }

    async fn append_message(&self, record: MessageRecord) -> Result<()> {
        let new_message = Object::builder(
            CLASS_MESSAGE,
            serde_json::to_value(&record).context("Failed to serialize record")?,
        )
        .build();

        self.client
            .objects
            .create(&new_message, None)
            .await
            .context("Failed to append message")?;
        Ok(())
    }

    async fn messages(
        &self,
        latest_n: Option<usize>,
        before: Option<u64>,
    ) -> Result<Vec<MessageRecord>> {
        let mut query_builder = GetQuery::builder(
            CLASS_MESSAGE,
            vec!["role", "content", "timestamp", "responseId", "session", "usage"],
        );
        
        if let Some(limit) = latest_n {
            query_builder = query_builder.with_limit(limit as u32);
        }
        
        if let Some(b) = before {
            query_builder = query_builder.with_where(
                &json!({
                    "path": ["timestamp"],
                    "operator": "LessThan",
                    "valueNumber": b,
                })
                .to_string(),
            );
        }
        
        let query = query_builder
            .with_sort("timestamp:asc")
            .build();

        let result = self
            .client
            .query
            .get(query)
            .await
            .context("Failed to get messages")?;

        let messages = extract_objects(&result, CLASS_MESSAGE)
            .unwrap_or(&vec![])
            .iter()
            .cloned()
            .filter_map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| {
                        warn!("Failed to deserialize record: {e}");
                        e
                    })
                    .ok()
            })
            .collect();
        Ok(messages)
    }

    async fn last_response_id(&self) -> Result<Option<String>> {
        let query = GetQuery::builder(CLASS_MESSAGE, vec!["responseId", "timestamp"])
            .with_where(
                &json!({
                    "path": ["role"],
                    "operator": "Equal",
                    "valueString": "assistant",
                })
                .to_string(),
            )
            .with_sort("timestamp:desc")
            .with_limit(1)
            .build();

        let result = self
            .client
            .query
            .get(query)
            .await
            .context("Failed to get last response ID")?;

        Ok(extract_objects(&result, CLASS_MESSAGE)
            .and_then(|messages| messages.first())
            .and_then(|obj| obj.get("responseId"))
            .and_then(|val| val.as_str().map(|s| s.to_string())))
    }

    async fn append_memory(&self, record: MemoryRecord) -> Result<()> {
        let new_memory = Object::builder(
            CLASS_MEMORY,
            serde_json::to_value(&record).context("Failed to serizalize record")?,
        )
        .build();
        self.client
            .objects
            .create(&new_memory, None)
            .await
            .context("Failed to append memory")?;
        Ok(())
    }

    async fn memories(&self, query: &str) -> Result<Vec<MemoryRecord>> {
        let query = GetQuery::builder(CLASS_MEMORY, vec!["content", "timestamp"])
            .with_near_text(query)
            .build();
        let result = self
            .client
            .query
            .get(query)
            .await
            .context("Failed to get memories")?;

        let memories = extract_objects(&result, CLASS_MEMORY)
            .unwrap_or(&vec![])
            .iter()
            .cloned()
            .filter_map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| {
                        warn!("Failed to deserialize record: {e}");
                        e
                    })
                    .ok()
            })
            .collect();
        Ok(memories)
    }

    async fn append_schedule(&self, expression: String, message: String) -> Result<()> {
        let new_schedule = Object::builder(
            CLASS_SCHEDULE,
            json!({
                "expression": expression,
                "message": message,
            }),
        )
        .build();
        self.client
            .objects
            .create(&new_schedule, None)
            .await
            .context("Failed to append schedule")?;
        Ok(())
    }

    async fn remove_schedule(&self, expression: String, message: String) -> Result<usize> {
        let query = GetQuery::builder(CLASS_SCHEDULE, vec!["_additional { id }"])
            .with_where(
                &json!({
                    "operator": "And",
                    "operands": [
                        {
                            "path": ["expression"],
                            "operator": "Equal",
                            "valueString": expression,
                        },
                        {
                            "path": ["message"],
                            "operator": "Equal",
                            "valueString": message,
                        }
                    ]
                })
                .to_string(),
            )
            .build();
        let schedules = self
            .client
            .query
            .get(query)
            .await
            .context("Failed to get schedules")?;

        let mut deleted_count = 0;
        if let Some(schedules) = extract_objects(&schedules, CLASS_SCHEDULE) {
            for schedule in schedules {
                if let Some(id) = extract_additional_id(schedule) {
                    if self
                        .client
                        .objects
                        .delete(CLASS_SCHEDULE, &id, None, None)
                        .await
                        .is_ok()
                    {
                        deleted_count += 1;
                    }
                }
            }
        }

        Ok(deleted_count)
    }

    async fn schedules(&self) -> Result<Vec<ScheduleRecord>> {
        let query = GetQuery::builder(CLASS_SCHEDULE, vec!["expression", "message"]).build();
        let result = self
            .client
            .query
            .get(query)
            .await
            .context("Failed to get schedules")?;

        let schedules = extract_objects(&result, CLASS_SCHEDULE)
            .unwrap_or(&vec![])
            .iter()
            .cloned()
            .filter_map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| {
                        warn!("Failed to deserialize record: {e}");
                        e
                    })
                    .ok()
            })
            .collect();
        Ok(schedules)
    }
}

fn extract_objects<'a>(
    value: &'a serde_json::Value,
    class_name: &str,
) -> Option<&'a Vec<serde_json::Value>> {
    value.get("data")?.get("Get")?.get(class_name)?.as_array()
}

fn extract_additional_id(value: &serde_json::Value) -> Option<Uuid> {
    value
        .get("_additional")?
        .get("id")?
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
}
