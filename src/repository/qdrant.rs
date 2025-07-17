use anyhow::Result;
use async_trait::async_trait;
use qdrant_client::qdrant::value::Kind;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct, QueryPointsBuilder,
    SearchParamsBuilder, SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

use super::Repository;
use crate::adapter::embedding::EmbeddingService;
use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, SessionId};
use crate::common::schedule::ScheduleRecord;

const COLLECTION_SESSION: &str = "session";
const COLLECTION_MEMORIES: &str = "memories";
const COLLECTION_MESSAGES: &str = "messages";

pub struct QdrantRepository {
    client: Qdrant,
    embedding_service: Arc<EmbeddingService>,
}

impl QdrantRepository {
    pub async fn new(url: &str, embedding_service: Arc<EmbeddingService>) -> Result<Self> {
        let client = Qdrant::from_url(url).build()?;
        let repo = Self {
            client,
            embedding_service,
        };
        repo.initialize().await?;
        Ok(repo)
    }

    async fn initialize(&self) -> Result<()> {
        let response = self.client.list_collections().await?;
        let names: Vec<&String> = response.collections.iter().map(|c| &c.name).collect();

        // Initialize session collection
        if !names.iter().any(|n| *n == COLLECTION_SESSION) {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(COLLECTION_SESSION)
                        .vectors_config(VectorParamsBuilder::new(1, Distance::Cosine)),
                )
                .await?;
        }

        // Initialize memories collection
        if !names.iter().any(|n| *n == COLLECTION_MEMORIES) {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(COLLECTION_MEMORIES).vectors_config(
                        VectorParamsBuilder::new(
                            self.embedding_service.dimensions() as u64,
                            Distance::Cosine,
                        ),
                    ),
                )
                .await?;
        }

        // Initialize messages collection
        if !names.iter().any(|n| *n == COLLECTION_MESSAGES) {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(COLLECTION_MESSAGES)
                        .vectors_config(VectorParamsBuilder::new(1, Distance::Cosine)),
                )
                .await?;
        }

        Ok(())
    }

    async fn get_all_memories(&self) -> Result<Vec<MemoryRecord>> {
        let response = self
            .client
            .query(
                QueryPointsBuilder::new(COLLECTION_MEMORIES)
                    .with_payload(true)
                    .limit(100), // Limit to 100 memories
            )
            .await?;

        let mut memories = Vec::new();
        for point in response.result {
            let payload: Payload = point.payload.into();
            let memory: MemoryRecord = serde_json::from_value(payload.into())?;
            memories.push(memory);
        }

        // Sort by timestamp (oldest first)
        memories.sort_by_key(|m| m.timestamp);
        Ok(memories)
    }
}

#[async_trait]
impl Repository for QdrantRepository {
    async fn get_or_create_session(&self) -> Result<SessionId> {
        let mut response = self
            .client
            .query(
                QueryPointsBuilder::new(COLLECTION_SESSION)
                    .with_payload(true)
                    .params(SearchParamsBuilder::default().exact(true))
                    .limit(1),
            )
            .await?;

        if response.result.len() > 0 {
            let mut session = response.result.remove(0);
            if let Some(id_value) = session.payload.remove("id") {
                if let Some(Kind::StringValue(id)) = id_value.kind {
                    return Ok(id);
                } else {
                    warn!("invalid session payload: id is not string");
                }
            } else {
                warn!("invalid session payload: id not found");
            }
        }

        let session = Uuid::new_v4().simple().to_string();
        let mut payload = Payload::new();
        payload.insert("id", session.clone());
        self.client
            .upsert_points(UpsertPointsBuilder::new(
                COLLECTION_SESSION,
                vec![PointStruct::new(0, vec![0.0], payload)],
            ))
            .await?;

        Ok(session)
    }

    async fn has_session(&self) -> bool {
        let result = self
            .client
            .query(QueryPointsBuilder::new(COLLECTION_SESSION).limit(1))
            .await;

        match result {
            Ok(response) => !response.result.is_empty(),
            Err(_) => false,
        }
    }

    async fn clear_session(&self) -> Result<()> {
        self.client
            .delete_points(DeletePointsBuilder::new(COLLECTION_SESSION))
            .await?;

        Ok(())
    }

    async fn append_message(&self, record: MessageRecord) -> Result<()> {
        let payload = Payload::try_from(serde_json::to_value(&record)?)?;
        let point = PointStruct::new(record.timestamp, vec![0.0], payload);
        self.client
            .upsert_points(UpsertPointsBuilder::new(COLLECTION_MESSAGES, vec![point]))
            .await?;

        Ok(())
    }

    async fn messages(
        &self,
        latest_n: Option<usize>,
        before: Option<u64>,
    ) -> Result<Vec<MessageRecord>> {
        let response = self
            .client
            .query(
                QueryPointsBuilder::new(COLLECTION_MESSAGES)
                    .with_payload(true)
                    .limit(1000), // Limit to 1000 messages
            )
            .await?;

        let mut messages = Vec::new();
        for point in response.result {
            let payload: Payload = point.payload.into();
            let message: MessageRecord = serde_json::from_value(payload.into())?;
            // Filter by before timestamp
            if let Some(before_ts) = before {
                if message.timestamp >= before_ts {
                    continue;
                }
            }
            messages.push(message);
        }

        // Sort by timestamp (descending - newest first)
        messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Apply latest_n limit
        if let Some(n) = latest_n {
            messages.truncate(n);
        }

        // Reverse to get chronological order (oldest first)
        messages.reverse();
        Ok(messages)
    }

    async fn last_response_id(&self) -> Result<Option<String>> {
        // Get current session
        let current_session = match self.get_or_create_session().await {
            Ok(session) => session,
            Err(_) => return Ok(None),
        };

        // Get recent messages and find the latest response_id for current session
        let messages = self.messages(Some(50), None).await?; // Get latest 50 messages

        for message in messages.iter().rev() {
            if message.session == current_session && message.response_id.is_some() {
                return Ok(message.response_id.clone());
            }
        }

        Ok(None)
    }

    async fn append_memory(&self, record: MemoryRecord) -> Result<()> {
        // Generate embedding for the memory content
        let embedding = self.embedding_service.embed_memory(&record).await?;

        // Create payload with memory data
        let payload = Payload::try_from(serde_json::to_value(&record)?)?;

        // Create and upsert point
        let point = PointStruct::new(record.timestamp, embedding, payload);
        self.client
            .upsert_points(UpsertPointsBuilder::new(COLLECTION_MEMORIES, vec![point]))
            .await?;

        Ok(())
    }

    async fn memories(&self, query: &str) -> Result<Vec<MemoryRecord>> {
        if query.is_empty() {
            // If query is empty, return all memories ordered by timestamp
            return self.get_all_memories().await;
        }

        // Generate embedding for the query
        let query_embedding = self.embedding_service.embed_text(query).await?;

        // Perform vector search
        let response = self
            .client
            .search_points(
                SearchPointsBuilder::new(COLLECTION_MEMORIES, query_embedding, 10)
                    .limit(10)
                    .with_payload(true)
                    .score_threshold(0.5),
            )
            .await?;

        // Convert results to MemoryRecord
        let mut memories = Vec::new();
        for scored_point in response.result {
            let payload: Payload = scored_point.payload.into();
            let memory: MemoryRecord = serde_json::from_value(payload.into())?;
            memories.push(memory);
        }

        Ok(memories)
    }

    async fn append_schedule(&self, _expression: String, _message: String) -> Result<()> {
        todo!("append_schedule not implemented for QdrantRepository")
    }

    async fn remove_schedule(&self, _expression: String, _message: String) -> Result<usize> {
        todo!("remove_schedule not implemented for QdrantRepository")
    }

    async fn schedules(&self) -> Result<Vec<ScheduleRecord>> {
        todo!("schedules not implemented for QdrantRepository")
    }
}
