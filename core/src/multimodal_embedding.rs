use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;

use crate::input_ingress::{MediaAttachment, RouterInput};

const DEFAULT_GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Debug, Clone)]
pub(crate) struct GeminiMultimodalEmbeddingConfig {
    pub(crate) enabled: bool,
    pub(crate) model: String,
    pub(crate) output_dimensionality: usize,
}

#[derive(Clone)]
pub(crate) struct GeminiMultimodalEmbeddingClient {
    client: Client,
    api_key: String,
    model: String,
    api_base_url: String,
    output_dimensionality: Option<usize>,
}

pub(crate) enum EmbeddingTaskType {
    RetrievalQuery,
    RetrievalDocument,
}

impl EmbeddingTaskType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::RetrievalQuery => "RETRIEVAL_QUERY",
            Self::RetrievalDocument => "RETRIEVAL_DOCUMENT",
        }
    }
}

impl GeminiMultimodalEmbeddingClient {
    pub(crate) fn from_env(
        config: &GeminiMultimodalEmbeddingConfig,
    ) -> Result<Option<Self>, String> {
        if !config.enabled {
            return Ok(None);
        }
        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| {
            "GEMINI_API_KEY is required when router.multimodal_embedding.enabled=true".to_string()
        })?;
        let client = Client::builder()
            .build()
            .map_err(|err| format!("gemini multimodal http client init failed: {}", err))?;
        let api_base_url = std::env::var("GEMINI_API_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_GEMINI_API_BASE_URL.to_string());
        let output_dimensionality = if config.output_dimensionality == 0 {
            None
        } else {
            Some(config.output_dimensionality)
        };
        Ok(Some(Self {
            client,
            api_key,
            model: config.model.clone(),
            api_base_url,
            output_dimensionality,
        }))
    }

    pub(crate) async fn embed_text(
        &self,
        text: &str,
        task_type: EmbeddingTaskType,
    ) -> Result<Vec<f64>, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let parts = vec![GeminiPart::Text {
            text: trimmed.to_string(),
        }];
        self.embed_parts(parts, task_type).await
    }

    pub(crate) async fn embed_router_input(&self, input: &RouterInput) -> Result<Vec<f64>, String> {
        let mut parts = Vec::<GeminiPart>::new();
        if !input.trimmed_text().is_empty() {
            parts.push(GeminiPart::Text {
                text: input.trimmed_text().to_string(),
            });
        }
        parts.extend(input.images.iter().cloned().map(Into::into));
        parts.extend(input.audio.iter().cloned().map(Into::into));
        if parts.is_empty() {
            return Ok(Vec::new());
        }
        self.embed_parts(parts, EmbeddingTaskType::RetrievalQuery)
            .await
    }

    async fn embed_parts(
        &self,
        parts: Vec<GeminiPart>,
        task_type: EmbeddingTaskType,
    ) -> Result<Vec<f64>, String> {
        let url = format!(
            "{}/models/{}:embedContent?key={}",
            self.api_base_url.trim_end_matches('/'),
            self.model,
            self.api_key
        );
        let request = GeminiEmbedContentRequest {
            model: format!("models/{}", self.model),
            content: GeminiContent { parts },
            task_type: task_type.as_str().to_string(),
            output_dimensionality: self.output_dimensionality,
        };
        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|err| format!("gemini embed request failed: {}", err))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "gemini embed request failed with status {}: {}",
                status, body
            ));
        }
        let body = response
            .json::<GeminiEmbedContentResponse>()
            .await
            .map_err(|err| format!("gemini embed response decode failed: {}", err))?;
        Ok(body.embedding.values)
    }
}

#[derive(Serialize)]
struct GeminiEmbedContentRequest {
    model: String,
    content: GeminiContent,
    #[serde(rename = "taskType")]
    task_type: String,
    #[serde(
        rename = "outputDimensionality",
        skip_serializing_if = "Option::is_none"
    )]
    output_dimensionality: Option<usize>,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
    InlineData(MediaAttachmentPart),
}

impl From<MediaAttachment> for MediaAttachmentPart {
    fn from(value: MediaAttachment) -> Self {
        Self {
            inline_data: GeminiInlineData {
                mime_type: value.mime_type,
                data: value.data,
            },
        }
    }
}

impl From<MediaAttachment> for GeminiPart {
    fn from(value: MediaAttachment) -> Self {
        Self::InlineData(value.into())
    }
}

#[derive(Serialize)]
struct MediaAttachmentPart {
    #[serde(rename = "inlineData")]
    inline_data: GeminiInlineData,
}

#[derive(Serialize)]
struct GeminiInlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
struct GeminiEmbedContentResponse {
    embedding: GeminiEmbedding,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f64>,
}
