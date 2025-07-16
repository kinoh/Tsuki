use anyhow::Result;
use async_trait::async_trait;
use qdrant_client::qdrant::value::Kind;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct, QueryPointsBuilder,
    SearchParamsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use tracing::warn;
use uuid::Uuid;

use super::Repository;
use crate::common::memory::MemoryRecord;
use crate::common::message::{MessageRecord, SessionId};
use crate::common::schedule::ScheduleRecord;

const COLLECTION_SESSION: &str = "session";

pub struct QdrantRepository {
    client: Qdrant,
}

impl QdrantRepository {
    pub async fn new(url: &str) -> Result<Self> {
        let client = Qdrant::from_url(url).build()?;
        Self::initialize(&client).await?;
        Ok(Self { client })
    }

    async fn initialize(client: &Qdrant) -> Result<()> {
        let response = client.list_collections().await?;
        let names: Vec<&String> = response.collections.iter().map(|c| &c.name).collect();

        if !names.iter().any(|n| *n == COLLECTION_SESSION) {
            client
                .create_collection(
                    CreateCollectionBuilder::new(COLLECTION_SESSION)
                        .vectors_config(VectorParamsBuilder::new(1, Distance::Cosine)),
                )
                .await?;
        }

        Ok(())
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

    async fn append_memory(&self, _record: MemoryRecord) -> Result<()> {
        todo!("append_memory not implemented for QdrantRepository")
    }

    async fn memories(&self, _query: &str) -> Result<Vec<MemoryRecord>> {
        todo!("memories not implemented for QdrantRepository")
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
