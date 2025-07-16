use anyhow::Result;
use openai_dive::v1::api::Client;
use openai_dive::v1::models::EmbeddingModel;
use openai_dive::v1::resources::embedding::{EmbeddingParametersBuilder, EmbeddingInput, EmbeddingOutput};

use crate::common::memory::MemoryRecord;

pub struct EmbeddingService {
    client: Client,
    model: EmbeddingModel,
    dimensions: usize,
}

impl EmbeddingService {
    pub async fn new(api_key: &str) -> Result<Self> {
        let client = Client::new(api_key.to_string());
        
        Ok(Self {
            client,
            model: EmbeddingModel::TextEmbedding3Small,
            dimensions: 1536,
        })
    }
    
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let parameters = EmbeddingParametersBuilder::default()
            .model("text-embedding-3-small")
            .input(EmbeddingInput::String(text.to_string()))
            .build()?;
        
        let response = self.client.embeddings().create(parameters).await?;
        
        if let Some(embedding) = response.data.first() {
            match &embedding.embedding {
                EmbeddingOutput::Float(vec) => Ok(vec.iter().map(|&x| x as f32).collect()),
                EmbeddingOutput::Base64(_) => {
                    anyhow::bail!("Base64 encoding not supported")
                }
            }
        } else {
            anyhow::bail!("No embedding returned from OpenAI API")
        }
    }
    
    pub async fn embed_memory(&self, record: &MemoryRecord) -> Result<Vec<f32>> {
        let combined_text = record.content.join(" ");
        self.embed_text(&combined_text).await
    }
    
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}