use anyhow::Result;
use async_trait::async_trait;
use qdrant_client::qdrant::value::Kind;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct, QueryPointsBuilder,
    SearchParamsBuilder, UpsertPointsBuilder, VectorParamsBuilder, SearchPointsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use std::collections::HashMap;
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

pub struct QdrantRepository {
    client: Qdrant,
    embedding_service: Arc<EmbeddingService>,
}

impl QdrantRepository {
    pub async fn new(url: &str, embedding_service: Arc<EmbeddingService>) -> Result<Self> {
        let client = Qdrant::from_url(url).build()?;
        let repo = Self { client, embedding_service };
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
                    CreateCollectionBuilder::new(COLLECTION_MEMORIES)
                        .vectors_config(VectorParamsBuilder::new(
                            self.embedding_service.dimensions() as u64,
                            Distance::Cosine
                        )),
                )
                .await?;
        }

        Ok(())
    }
    
    async fn get_all_memories(&self) -> Result<Vec<MemoryRecord>> {
        let response = self.client
            .query(
                QueryPointsBuilder::new(COLLECTION_MEMORIES)
                    .with_payload(true)
                    .limit(100), // Limit to 100 memories
            )
            .await?;
        
        let mut memories = Vec::new();
        for point in response.result {
            if let Some(memory) = self.point_to_memory_record(point.payload.into())? {
                memories.push(memory);
            }
        }
        
        // Sort by timestamp (oldest first)
        memories.sort_by_key(|m| m.timestamp);
        Ok(memories)
    }
    
    fn point_to_memory_record(&self, payload: HashMap<String, qdrant_client::qdrant::Value>) -> Result<Option<MemoryRecord>> {
        let timestamp = match payload.get("timestamp") {
            Some(value) => match &value.kind {
                Some(Kind::IntegerValue(i)) => *i as u64,
                _ => return Ok(None),
            },
            None => return Ok(None),
        };
        
        let content = match payload.get("content") {
            Some(value) => match &value.kind {
                Some(Kind::ListValue(list)) => {
                    let mut strings = Vec::new();
                    for item in &list.values {
                        if let Some(Kind::StringValue(s)) = &item.kind {
                            strings.push(s.clone());
                        }
                    }
                    strings
                },
                _ => return Ok(None),
            },
            None => return Ok(None),
        };
        
        Ok(Some(MemoryRecord { timestamp, content }))
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

    async fn append_message(&self, _record: MessageRecord) -> Result<()> {
        todo!("append_message not implemented for QdrantRepository")
    }

    async fn messages(
        &self,
        _latest_n: Option<usize>,
        _before: Option<u64>,
    ) -> Result<Vec<MessageRecord>> {
        todo!("messages not implemented for QdrantRepository")
    }

    async fn last_response_id(&self) -> Result<Option<String>> {
        todo!("last_response_id not implemented for QdrantRepository")
    }

    async fn append_memory(&self, record: MemoryRecord) -> Result<()> {
        // Generate embedding for the memory content
        let embedding = self.embedding_service.embed_memory(&record).await?;
        
        // Create payload with memory data
        let mut payload = Payload::new();
        payload.insert("timestamp", record.timestamp as i64);
        payload.insert("content", record.content.clone());
        
        // Create and upsert point
        let point = PointStruct::new(record.timestamp, embedding, payload);
        self.client
            .upsert_points(UpsertPointsBuilder::new(
                COLLECTION_MEMORIES,
                vec![point],
            ))
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
        let response = self.client
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
            if let Some(memory) = self.point_to_memory_record(scored_point.payload)? {
                memories.push(memory);
            }
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
