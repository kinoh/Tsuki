use async_trait::async_trait;
use neo4rs::{query, Graph};
use safetensors::{tensor::TensorView, Dtype, SafeTensors};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use time::{OffsetDateTime, UtcOffset};
use tokenizers::Tokenizer;

use crate::conversation_recall_store::{
    conversation_recall_text, ConversationRecallCandidate, ConversationRecallStore,
};
use crate::input_ingress::RouterInput;
use crate::multimodal_embedding::{
    EmbeddingTaskType, GeminiMultimodalEmbeddingClient, GeminiMultimodalEmbeddingConfig,
};

const DEFAULT_VALENCE: f64 = 0.0;
const DEFAULT_AROUSAL_LEVEL: f64 = 0.0;
const INITIAL_AROUSAL_UPSERT: f64 = 0.5;
const INITIAL_AROUSAL_INDIRECT: f64 = 0.25;
const DEFAULT_ACCESSED_AT: i64 = 0;
const DEFAULT_RELATION_WEIGHT: f64 = 0.25;
const RELATION_WEIGHT_ALPHA: f64 = 0.2;
const REVERSE_PENALTY: f64 = 0.5;
const DEFAULT_VECTOR_SEARCH_RAW_LIMIT_MULTIPLIER: usize = 4;
const DEFAULT_VECTOR_SEARCH_SEMANTIC_WEIGHT: f64 = 0.75;
const DEFAULT_VECTOR_SEARCH_AROUSAL_WEIGHT: f64 = 0.25;
const DEFAULT_VECTOR_INDEX_NAME: &str = "concept_embedding_idx";
const DEFAULT_MULTIMODAL_VECTOR_INDEX_NAME: &str = "concept_embedding_multimodal_idx";
const DEFAULT_CONVERSATION_VECTOR_INDEX_NAME: &str = "conversation_event_embedding_idx";
const DEFAULT_VECTOR_INDEX_CAPACITY: usize = 200_000;
const DEFAULT_VECTOR_INDEX_SCALAR_KIND: &str = "f32";
const DEFAULT_VECTOR_INDEX_RESIZE_COEFFICIENT: usize = 2;
const DEFAULT_CONCEPT_EMBEDDING_MODEL_DIR: &str =
    "/opt/tsuki/models/quantized-stable-static-embedding-fast-retrieval-mrl-ja";

#[async_trait]
pub(crate) trait ConceptGraphActivationReader: Send + Sync {
    async fn concept_search(&self, input_text: &str, limit: usize) -> Result<Vec<String>, String>;
    async fn concept_search_multimodal(
        &self,
        input: &RouterInput,
        limit: usize,
    ) -> Result<Vec<String>, String>;
    async fn active_nodes(&self, limit: usize) -> Result<Vec<ActiveGraphNode>, String>;
    async fn concept_activation(&self, concepts: &[String])
        -> Result<HashMap<String, f64>, String>;
    async fn visible_skills(
        &self,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<VisibleSkill>, String>;
}

#[async_trait]
pub(crate) trait ConceptGraphDebugReader: Send + Sync {
    async fn debug_health(&self) -> Result<Value, String>;
    async fn debug_counts(&self) -> Result<Value, String>;
    async fn debug_concept_search(
        &self,
        query: Option<String>,
        limit: usize,
    ) -> Result<Vec<Value>, String>;
    async fn debug_concept_detail(&self, concept: String) -> Result<Option<Value>, String>;
    async fn debug_episode_search(
        &self,
        query: Option<String>,
        limit: usize,
    ) -> Result<Vec<Value>, String>;
    async fn debug_episode_detail(&self, episode: String) -> Result<Option<Value>, String>;
    async fn debug_relation_search(
        &self,
        query: Option<String>,
        limit: usize,
    ) -> Result<Vec<Value>, String>;
}

#[async_trait]
pub(crate) trait ConceptGraphOps: Send + Sync {
    async fn concept_upsert(&self, concept: String) -> Result<Value, String>;
    async fn skill_index_upsert(
        &self,
        skill_name: String,
        summary: String,
        body_state_key: String,
        enabled: bool,
    ) -> Result<Value, String>;
    async fn skill_index_replace_triggers(
        &self,
        skill_name: String,
        trigger_concepts: Vec<String>,
    ) -> Result<Value, String>;
    async fn update_affect(&self, target: String, valence_delta: f64) -> Result<Value, String>;
    async fn activate_related_submodules(
        &self,
        concepts: Vec<String>,
    ) -> Result<HashMap<String, f64>, String>;
    async fn activate_related_skills(
        &self,
        concepts: Vec<String>,
    ) -> Result<HashMap<String, f64>, String>;
    async fn dampen_concept_arousal(&self, concept: String, ratio: f64) -> Result<Value, String>;
    async fn episode_add(&self, summary: String, concepts: Vec<String>) -> Result<Value, String>;
    async fn relation_add(
        &self,
        from: String,
        to: String,
        relation_type: String,
    ) -> Result<Value, String>;
    /// When `dry_run` is true, arousal updates are skipped. For debug/testing only.
    async fn recall_query(&self, seeds: Vec<String>, max_hop: u32, dry_run: bool) -> Result<Value, String>;
}

pub(crate) trait ConceptGraphStore:
    ConceptGraphActivationReader + ConceptGraphOps + ConceptGraphDebugReader
{
}
impl<T> ConceptGraphStore for T where
    T: ConceptGraphActivationReader + ConceptGraphOps + ConceptGraphDebugReader
{
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RelationType {
    IsA,
    PartOf,
    Evokes,
}

#[derive(Debug, Clone)]
struct ConceptState {
    valence: f64,
    arousal_level: f64,
    accessed_at: i64,
}

#[derive(Debug, Clone)]
struct EpisodeState {
    valence: f64,
    arousal_level: f64,
    accessed_at: i64,
}

#[derive(Debug, Clone)]
struct RelationEdge {
    from: String,
    to: String,
    relation_type: String,
    weight: f64,
}

#[derive(Debug, Clone)]
struct EpisodeEntry {
    summary: String,
    valence: f64,
    weight: f64,
}

#[derive(Debug, Clone)]
struct Proposition {
    text: String,
    score: f64,
    valence: Option<f64>,
}

#[derive(Debug, Clone)]
struct VectorCandidate {
    name: String,
    semantic: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveGraphNode {
    pub(crate) label: String,
    pub(crate) arousal: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct VisibleSkill {
    pub(crate) name: String,
    pub(crate) summary: String,
    pub(crate) body_state_key: String,
    pub(crate) score: f64,
}

#[derive(Debug, Clone)]
struct EmbeddingConfig {
    model_dir: PathBuf,
    vector_index_name: String,
    vector_index_capacity: usize,
    vector_index_scalar_kind: String,
    vector_index_resize_coefficient: usize,
    vector_search_raw_limit_multiplier: usize,
    vector_search_semantic_weight: f64,
    vector_search_arousal_weight: f64,
}

#[derive(Clone)]
struct MultimodalEmbeddingState {
    client: GeminiMultimodalEmbeddingClient,
    vector_index_name: String,
}

#[derive(Debug)]
struct SseEmbeddingModel {
    tokenizer: Tokenizer,
    hidden_dim: usize,
    vocab_size: usize,
    packed: Vec<u8>,
    scales: Vec<f32>,
    alpha: Vec<f32>,
    beta: Vec<f32>,
    bias: Vec<f32>,
}

impl EmbeddingConfig {
    fn from_defaults() -> Result<Self, String> {
        let model_dir = PathBuf::from(DEFAULT_CONCEPT_EMBEDDING_MODEL_DIR);
        let vector_index_name = DEFAULT_VECTOR_INDEX_NAME.to_string();
        let vector_index_capacity = DEFAULT_VECTOR_INDEX_CAPACITY.max(1);
        let vector_index_scalar_kind = DEFAULT_VECTOR_INDEX_SCALAR_KIND.to_string();
        let vector_index_resize_coefficient = DEFAULT_VECTOR_INDEX_RESIZE_COEFFICIENT.max(1);
        let vector_search_raw_limit_multiplier = DEFAULT_VECTOR_SEARCH_RAW_LIMIT_MULTIPLIER.max(1);
        let semantic_weight = DEFAULT_VECTOR_SEARCH_SEMANTIC_WEIGHT.clamp(0.0, 1.0);
        let arousal_weight = DEFAULT_VECTOR_SEARCH_AROUSAL_WEIGHT.clamp(0.0, 1.0);
        if !model_dir.exists() {
            return Err(format!(
                "embedding model directory not found: {}",
                model_dir.display()
            ));
        }
        if semantic_weight <= 0.0 && arousal_weight <= 0.0 {
            return Err(
                "invalid vector ranking weights: both semantic/arousal are zero".to_string(),
            );
        }
        Ok(Self {
            model_dir,
            vector_index_name,
            vector_index_capacity,
            vector_index_scalar_kind,
            vector_index_resize_coefficient,
            vector_search_raw_limit_multiplier,
            vector_search_semantic_weight: semantic_weight,
            vector_search_arousal_weight: arousal_weight,
        })
    }
}

impl SseEmbeddingModel {
    fn load(model_dir: &std::path::Path) -> Result<Self, String> {
        let tokenizer_path = model_dir.join("tokenizer.json");
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|err| format!("tokenizer load: {}", err))?;

        let rest_path = model_dir.join("model_rest.safetensors");
        let rest_bytes = fs::read(rest_path).map_err(|err| format!("read model_rest: {}", err))?;
        let safetensors = SafeTensors::deserialize(&rest_bytes)
            .map_err(|err| format!("parse model_rest.safetensors: {}", err))?;

        let alpha = read_f32_tensor(&safetensors, "dyt.alpha")?;
        let beta = read_f32_tensor(&safetensors, "dyt.beta")?;
        let bias = read_f32_tensor(&safetensors, "dyt.bias")?;
        if alpha.len() != beta.len() || alpha.len() != bias.len() {
            return Err("invalid dyt tensor sizes".to_string());
        }
        let hidden_dim = alpha.len();
        if hidden_dim == 0 || hidden_dim % 2 != 0 {
            return Err(format!(
                "hidden dimension must be positive and even, got {}",
                hidden_dim
            ));
        }

        let emb_path = model_dir.join("embedding.q4_k_m.bin");
        let emb_bytes = fs::read(emb_path).map_err(|err| format!("read embedding: {}", err))?;
        let bytes_per_row = hidden_dim / 2 + 4;
        if emb_bytes.is_empty() || emb_bytes.len() % bytes_per_row != 0 {
            return Err(format!(
                "invalid embedding binary size: {} bytes (bytes_per_row={})",
                emb_bytes.len(),
                bytes_per_row
            ));
        }
        let vocab_size = emb_bytes.len() / bytes_per_row;
        let packed_size = vocab_size * hidden_dim / 2;
        let packed = emb_bytes[..packed_size].to_vec();
        let scale_bytes = &emb_bytes[packed_size..];
        let scales = scale_bytes
            .chunks_exact(4)
            .map(|chunk| {
                let mut bytes = [0u8; 4];
                bytes.copy_from_slice(chunk);
                f32::from_le_bytes(bytes)
            })
            .collect::<Vec<_>>();
        if scales.len() != vocab_size {
            return Err(format!(
                "invalid scale length: expected {}, got {}",
                vocab_size,
                scales.len()
            ));
        }
        Ok(Self {
            tokenizer,
            hidden_dim,
            vocab_size,
            packed,
            scales,
            alpha,
            beta,
            bias,
        })
    }

    fn encode(&self, text: &str) -> Result<Vec<f64>, String> {
        let encoding = self
            .tokenizer
            .encode(text, false)
            .map_err(|err| format!("tokenize failed for '{}': {}", text, err))?;
        let ids = encoding.get_ids();
        if ids.is_empty() {
            return Ok(vec![0.0; self.hidden_dim]);
        }
        let mut acc = vec![0.0_f64; self.hidden_dim];
        for token_id in ids {
            self.add_dequantized_row(*token_id as usize, &mut acc)?;
        }
        let denom = ids.len() as f64;
        for value in &mut acc {
            *value /= denom;
        }
        for (idx, value) in acc.iter_mut().enumerate() {
            let x = self.alpha[idx] as f64 * *value + self.bias[idx] as f64;
            *value = self.beta[idx] as f64 * x.tanh();
        }
        ActivationConceptGraphStore::l2_normalize(&mut acc);
        Ok(acc)
    }

    fn add_dequantized_row(&self, token_id: usize, out: &mut [f64]) -> Result<(), String> {
        if token_id >= self.vocab_size {
            return Err(format!(
                "token id out of range: id={} vocab_size={}",
                token_id, self.vocab_size
            ));
        }
        let row_scale = self.scales[token_id] as f64;
        let row_start = token_id * (self.hidden_dim / 2);
        for pair_idx in 0..(self.hidden_dim / 2) {
            let byte = self.packed[row_start + pair_idx];
            let hi = ((byte >> 4) & 0x0F) as f64;
            let lo = (byte & 0x0F) as f64;
            let dim_hi = pair_idx * 2;
            let dim_lo = dim_hi + 1;
            out[dim_hi] += ((hi / 7.5) - 1.0) * row_scale;
            out[dim_lo] += ((lo / 7.5) - 1.0) * row_scale;
        }
        Ok(())
    }
}

fn read_f32_tensor(safetensors: &SafeTensors<'_>, name: &str) -> Result<Vec<f32>, String> {
    let tensor = safetensors
        .tensor(name)
        .map_err(|err| format!("missing tensor {}: {}", name, err))?;
    tensor_view_to_f32_vec(&tensor)
}

fn tensor_view_to_f32_vec(view: &TensorView<'_>) -> Result<Vec<f32>, String> {
    if view.dtype() != Dtype::F32 {
        return Err(format!("expected f32 tensor, got {:?}", view.dtype()));
    }
    let bytes = view.data();
    if bytes.len() % 4 != 0 {
        return Err(format!("invalid f32 tensor byte size: {}", bytes.len()));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| {
            let mut raw = [0u8; 4];
            raw.copy_from_slice(chunk);
            f32::from_le_bytes(raw)
        })
        .collect())
}

fn normalize_index_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn normalize_scalar_kind(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "f16" => Ok("f16"),
        "f32" => Ok("f32"),
        other => Err(format!(
            "invalid scalar kind '{}': expected f16 or f32",
            other
        )),
    }
}

pub(crate) struct ActivationConceptGraphStore {
    graph: Arc<Graph>,
    arousal_tau_ms: f64,
    embedding: Arc<SseEmbeddingModel>,
    embedding_config: EmbeddingConfig,
    multimodal_embedding: Option<MultimodalEmbeddingState>,
}

impl ActivationConceptGraphStore {
    fn is_skill_name(name: &str) -> bool {
        name.starts_with("skill:")
    }

    fn skill_body_state_key(name: &str, body_state_key: &str) -> String {
        Self::normalize_non_empty(body_state_key).unwrap_or_else(|| name.to_string())
    }

    fn skill_summary(name: &str, summary: &str) -> String {
        if let Some(value) = Self::normalize_non_empty(summary) {
            return value;
        }
        if let Some(value) = name.strip_prefix("skill:") {
            return value.to_string();
        }
        name.to_string()
    }

    fn new(
        graph: Arc<Graph>,
        arousal_tau_ms: f64,
        embedding: Arc<SseEmbeddingModel>,
        embedding_config: EmbeddingConfig,
        multimodal_embedding: Option<MultimodalEmbeddingState>,
    ) -> Self {
        Self {
            graph,
            arousal_tau_ms: arousal_tau_ms.max(1.0),
            embedding,
            embedding_config,
            multimodal_embedding,
        }
    }

    pub(crate) async fn connect(
        uri: String,
        user: String,
        password: String,
        arousal_tau_ms: f64,
        multimodal_config: Option<GeminiMultimodalEmbeddingConfig>,
    ) -> Result<Self, String> {
        let embedding_config = EmbeddingConfig::from_defaults()?;
        let embedding = Arc::new(SseEmbeddingModel::load(
            embedding_config.model_dir.as_path(),
        )?);
        let embedding_dim = embedding.hidden_dim;
        let graph = Graph::new(uri, user, password).map_err(|err| err.to_string())?;
        Self::ensure_constraints(&graph).await?;
        Self::ensure_conversation_event_constraints(&graph).await?;
        Self::ensure_vector_index(&graph, &embedding_config, embedding_dim).await?;
        Self::ensure_conversation_event_vector_index(&graph, embedding_dim).await?;
        let multimodal_embedding = if let Some(config) = multimodal_config {
            if let Some(client) = GeminiMultimodalEmbeddingClient::from_env(&config)? {
                let dim = if config.output_dimensionality > 0 {
                    config.output_dimensionality
                } else {
                    let probe = client
                        .embed_text("concept probe", EmbeddingTaskType::RetrievalDocument)
                        .await?;
                    if probe.is_empty() {
                        return Err(
                            "gemini multimodal embedding returned empty vector".to_string(),
                        );
                    }
                    probe.len()
                };
                Self::ensure_named_vector_index(
                    &graph,
                    DEFAULT_MULTIMODAL_VECTOR_INDEX_NAME,
                    "Concept",
                    "embedding_multimodal",
                    dim,
                    DEFAULT_VECTOR_INDEX_CAPACITY,
                    DEFAULT_VECTOR_INDEX_RESIZE_COEFFICIENT,
                    DEFAULT_VECTOR_INDEX_SCALAR_KIND,
                )
                .await?;
                Some(MultimodalEmbeddingState {
                    client,
                    vector_index_name: DEFAULT_MULTIMODAL_VECTOR_INDEX_NAME.to_string(),
                })
            } else {
                None
            }
        } else {
            None
        };
        let store = Self::new(
            Arc::new(graph),
            arousal_tau_ms,
            embedding,
            embedding_config,
            multimodal_embedding,
        );
        if store.multimodal_embedding.is_some() {
            let (embedded, total) = store.backfill_multimodal_concept_embeddings(None).await?;
            println!(
                "MULTIMODAL_CONCEPT_BACKFILL embedded={} total={}",
                embedded, total
            );
        }
        Ok(store)
    }

    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0)
    }

    fn normalize_non_empty(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn clamp(v: f64, min: f64, max: f64) -> f64 {
        v.max(min).min(max)
    }

    fn clamp_limit(limit: usize, default_limit: usize, max_limit: usize) -> usize {
        let value = if limit == 0 { default_limit } else { limit };
        value.max(1).min(max_limit)
    }

    fn hop_decay(hop: u32) -> f64 {
        0.5_f64.powi((hop.saturating_sub(1)) as i32)
    }

    fn round_score(value: f64) -> f64 {
        if !value.is_finite() {
            return value;
        }
        let factor = 1_000_000.0_f64;
        (value * factor).round() / factor
    }

    fn arousal(&self, level: f64, accessed_at: i64, now: i64) -> f64 {
        let delta_ms = (now - accessed_at).max(0) as f64;
        let decay = (-delta_ms / self.arousal_tau_ms).exp();
        level * decay
    }

    fn parse_relation_type(raw: &str) -> Option<RelationType> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "is-a" => Some(RelationType::IsA),
            "part-of" => Some(RelationType::PartOf),
            "evokes" => Some(RelationType::Evokes),
            _ => None,
        }
    }

    fn map_relation_label(value: &RelationType) -> &'static str {
        match value {
            RelationType::IsA => "IS_A",
            RelationType::PartOf => "PART_OF",
            RelationType::Evokes => "EVOKES",
        }
    }

    fn render_relation_type(value: &str) -> &'static str {
        match value {
            "IS_A" => "is-a",
            "PART_OF" => "part-of",
            "EVOKES" => "evokes",
            _ => "evokes",
        }
    }

    async fn ensure_constraints(graph: &Graph) -> Result<(), String> {
        let q = query("CREATE CONSTRAINT ON (c:Concept) ASSERT c.name IS UNIQUE");
        match graph.execute(q).await {
            Ok(mut result) => {
                let _ = result.next().await;
                Ok(())
            }
            Err(err) => {
                let message = err.to_string();
                if message.contains("already exists") {
                    Ok(())
                } else {
                    Err(message)
                }
            }
        }
    }

    async fn ensure_conversation_event_constraints(graph: &Graph) -> Result<(), String> {
        let q = query("CREATE CONSTRAINT ON (e:ConversationEvent) ASSERT e.event_id IS UNIQUE");
        match graph.execute(q).await {
            Ok(mut result) => {
                let _ = result.next().await;
                Ok(())
            }
            Err(err) => {
                let message = err.to_string();
                if message.contains("already exists") {
                    Ok(())
                } else {
                    Err(message)
                }
            }
        }
    }

    async fn ensure_vector_index(
        graph: &Graph,
        config: &EmbeddingConfig,
        dimension: usize,
    ) -> Result<(), String> {
        Self::ensure_named_vector_index(
            graph,
            config.vector_index_name.as_str(),
            "Concept",
            "embedding",
            dimension,
            config.vector_index_capacity,
            config.vector_index_resize_coefficient,
            config.vector_index_scalar_kind.as_str(),
        )
        .await
    }

    async fn ensure_conversation_event_vector_index(
        graph: &Graph,
        dimension: usize,
    ) -> Result<(), String> {
        Self::ensure_named_vector_index(
            graph,
            DEFAULT_CONVERSATION_VECTOR_INDEX_NAME,
            "ConversationEvent",
            "embedding",
            dimension,
            DEFAULT_VECTOR_INDEX_CAPACITY,
            DEFAULT_VECTOR_INDEX_RESIZE_COEFFICIENT,
            DEFAULT_VECTOR_INDEX_SCALAR_KIND,
        )
        .await
    }

    async fn ensure_named_vector_index(
        graph: &Graph,
        index_name_raw: &str,
        label: &str,
        property_name: &str,
        dimension: usize,
        capacity: usize,
        resize_coefficient: usize,
        scalar_kind_raw: &str,
    ) -> Result<(), String> {
        let mut has_index = false;
        let mut result = graph
            .execute(query("SHOW VECTOR INDEX INFO;"))
            .await
            .map_err(|err| format!("SHOW VECTOR INDEX INFO failed: {}", err))?;
        while let Ok(Some(row)) = result.next().await {
            let index_name: String = row.get("index_name").unwrap_or_default();
            if index_name == index_name_raw {
                has_index = true;
                let existing_dimension: i64 = row.get("dimension").unwrap_or(0);
                if existing_dimension != dimension as i64 {
                    return Err(format!(
                        "vector index '{}' has incompatible dimension: expected {}, got {}",
                        index_name_raw, dimension, existing_dimension
                    ));
                }
                break;
            }
        }
        if has_index {
            return Ok(());
        }
        let index_name = normalize_index_name(index_name_raw)
            .ok_or_else(|| format!("invalid vector index name: {}", index_name_raw))?;
        let scalar_kind = normalize_scalar_kind(scalar_kind_raw)?;
        let cypher = format!(
            "CREATE VECTOR INDEX {index_name} ON :{label}({property_name}) WITH CONFIG {{\"dimension\": {dimension}, \"capacity\": {capacity}, \"metric\": \"cos\", \"resize_coefficient\": {resize_coefficient}, \"scalar_kind\": \"{scalar_kind}\"}};",
            index_name = index_name,
            label = label,
            property_name = property_name,
            dimension = dimension,
            capacity = capacity,
            resize_coefficient = resize_coefficient,
            scalar_kind = scalar_kind,
        );
        let mut stream = graph
            .execute(query(cypher.as_str()))
            .await
            .map_err(|err| format!("CREATE VECTOR INDEX failed: {}", err))?;
        let _ = stream.next().await;
        Ok(())
    }

    fn l2_normalize(values: &mut [f64]) {
        let norm_sq = values.iter().map(|value| value * value).sum::<f64>();
        if norm_sq <= 0.0 {
            return;
        }
        let norm = norm_sq.sqrt();
        for value in values {
            *value /= norm;
        }
    }

    fn to_embedding_property_vector(embedding: &[f64]) -> Vec<f64> {
        embedding
            .iter()
            .map(|value| Self::round_score(*value))
            .collect::<Vec<_>>()
    }

    async fn upsert_concept_embedding_for_text(
        &self,
        concept: &str,
        embedding_text: &str,
    ) -> Result<(), String> {
        let embedding = self.embedding.encode(embedding_text)?;
        let embedding = Self::to_embedding_property_vector(&embedding);
        let mut stream = self
            .graph
            .execute(
                query(
                    "MATCH (c:Concept {name: $name})
                     SET c.embedding = $embedding
                     RETURN c.name AS name",
                )
                .param("name", concept)
                .param("embedding", embedding),
            )
            .await
            .map_err(|err| format!("upsert concept embedding failed for {}: {}", concept, err))?;
        let _ = stream.next().await;
        Ok(())
    }

    async fn upsert_concept_embedding(&self, concept: &str) -> Result<(), String> {
        self.upsert_concept_embedding_for_text(concept, concept)
            .await
    }

    async fn upsert_concept_multimodal_embedding(&self, concept: &str) -> Result<(), String> {
        let Some(multimodal) = &self.multimodal_embedding else {
            return Ok(());
        };
        let embedding = multimodal
            .client
            .embed_text(concept, EmbeddingTaskType::RetrievalDocument)
            .await?;
        let embedding = Self::to_embedding_property_vector(&embedding);
        let mut stream = self
            .graph
            .execute(
                query(
                    "MATCH (c:Concept {name: $name})
                     SET c.embedding_multimodal = $embedding
                     RETURN c.name AS name",
                )
                .param("name", concept)
                .param("embedding", embedding),
            )
            .await
            .map_err(|err| {
                format!(
                    "upsert concept multimodal embedding failed for {}: {}",
                    concept, err
                )
            })?;
        let _ = stream.next().await;
        Ok(())
    }

    async fn vector_search_candidates(
        &self,
        input_text: &str,
        limit: usize,
    ) -> Result<Vec<VectorCandidate>, String> {
        let query_text = input_text.trim();
        if query_text.is_empty() {
            return Ok(Vec::new());
        }
        let query_embedding = self.embedding.encode(query_text)?;
        let query_embedding = Self::to_embedding_property_vector(&query_embedding);
        self.vector_search_candidates_by_embedding(
            self.embedding_config.vector_index_name.as_str(),
            query_embedding,
            limit,
        )
        .await
    }

    async fn vector_search_candidates_multimodal(
        &self,
        input: &RouterInput,
        limit: usize,
    ) -> Result<Vec<VectorCandidate>, String> {
        let Some(multimodal) = &self.multimodal_embedding else {
            return Ok(Vec::new());
        };
        let query_embedding = multimodal.client.embed_router_input(input).await?;
        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }
        let query_embedding = Self::to_embedding_property_vector(&query_embedding);
        self.vector_search_candidates_by_embedding(
            multimodal.vector_index_name.as_str(),
            query_embedding,
            limit,
        )
        .await
    }

    async fn vector_search_candidates_by_embedding(
        &self,
        index_name: &str,
        query_embedding: Vec<f64>,
        limit: usize,
    ) -> Result<Vec<VectorCandidate>, String> {
        let raw_limit = limit
            .max(1)
            .saturating_mul(self.embedding_config.vector_search_raw_limit_multiplier);
        let cypher = format!(
            "CALL vector_search.search(\"{}\", $limit, $embedding) YIELD node, similarity
             RETURN node.name AS name, similarity
             ORDER BY similarity DESC",
            index_name
        );
        let mut result = self
            .graph
            .execute(
                query(cypher.as_str())
                    .param("limit", raw_limit as i64)
                    .param("embedding", query_embedding),
            )
            .await
            .map_err(|err| format!("vector_search.search failed: {}", err))?;
        let mut candidates = Vec::<VectorCandidate>::new();
        let mut seen = HashSet::<String>::new();
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            let semantic = row.get::<f64>("similarity").unwrap_or(0.0);
            candidates.push(VectorCandidate {
                name,
                semantic: semantic.clamp(0.0, 1.0),
            });
            if candidates.len() >= raw_limit {
                break;
            }
        }
        Ok(candidates)
    }

    async fn backfill_multimodal_concept_embeddings(
        &self,
        limit: Option<usize>,
    ) -> Result<(usize, usize), String> {
        if self.multimodal_embedding.is_none() {
            return Ok((0, 0));
        }
        let mut names = Vec::<String>::new();
        let mut result = self
            .graph
            .execute(query(
                "MATCH (c:Concept) WHERE c.embedding_multimodal IS NULL RETURN c.name AS name ORDER BY name ASC",
            ))
            .await
            .map_err(|err| format!("concept name scan failed: {}", err))?;
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            names.push(name);
            if let Some(max) = limit {
                if names.len() >= max {
                    break;
                }
            }
        }
        let total = names.len();
        let mut embedded = 0usize;
        for name in names {
            self.upsert_concept_multimodal_embedding(name.as_str())
                .await?;
            embedded += 1;
        }
        Ok((embedded, total))
    }

    #[allow(dead_code)]
    pub(crate) async fn backfill_concept_embeddings(
        &self,
        limit: Option<usize>,
    ) -> Result<(usize, usize), String> {
        let mut names = Vec::<String>::new();
        let mut result = self
            .graph
            .execute(query(
                "MATCH (c:Concept) RETURN c.name AS name ORDER BY name ASC",
            ))
            .await
            .map_err(|err| format!("concept name scan failed: {}", err))?;
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            names.push(name);
            if let Some(max) = limit {
                if names.len() >= max {
                    break;
                }
            }
        }
        let mut updated = 0usize;
        let mut failed = 0usize;
        for name in &names {
            match self.upsert_concept_embedding(name.as_str()).await {
                Ok(_) => updated += 1,
                Err(err) => {
                    failed += 1;
                    eprintln!("EMBED_BACKFILL_ERROR concept={} error={}", name, err);
                }
            }
        }
        Ok((updated, failed))
    }

    fn local_date_yyyymmdd(now_ms: i64) -> String {
        let now = OffsetDateTime::from_unix_timestamp_nanos((now_ms as i128) * 1_000_000)
            .unwrap_or_else(|_| OffsetDateTime::UNIX_EPOCH);
        let local = UtcOffset::current_local_offset()
            .map(|offset| now.to_offset(offset))
            .unwrap_or(now);
        format!(
            "{:04}{:02}{:02}",
            local.year(),
            local.month() as u8,
            local.day()
        )
    }

    async fn concept_exists(&self, name: &str) -> Result<bool, String> {
        let q =
            query("MATCH (c:Concept {name: $name}) RETURN count(c) AS count").param("name", name);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        if let Ok(Some(row)) = result.next().await {
            let count: i64 = row.get("count").unwrap_or(0);
            Ok(count > 0)
        } else {
            Ok(false)
        }
    }

    async fn episode_exists(&self, name: &str) -> Result<bool, String> {
        let q =
            query("MATCH (e:Episode {name: $name}) RETURN count(e) AS count").param("name", name);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        if let Ok(Some(row)) = result.next().await {
            let count: i64 = row.get("count").unwrap_or(0);
            Ok(count > 0)
        } else {
            Ok(false)
        }
    }

    async fn ensure_concept(
        &self,
        concept: &str,
        now: i64,
        initial_arousal_level: f64,
    ) -> Result<(), String> {
        let q = query(
            "MERGE (c:Concept {name: $name})
             ON CREATE SET c.valence = $valence, c.arousal_level = $arousal_level, c.accessed_at = $accessed_at
             RETURN c.name AS name",
        )
        .param("name", concept)
        .param("valence", DEFAULT_VALENCE)
        .param("arousal_level", initial_arousal_level)
        .param("accessed_at", now);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let _ = result.next().await.map_err(|err| err.to_string())?;
        Ok(())
    }

    async fn next_episode_id(&self, base: &str) -> Result<String, String> {
        let mut candidate = base.to_string();
        let mut suffix = 2;
        loop {
            if !self.episode_exists(candidate.as_str()).await?
                && !self.concept_exists(candidate.as_str()).await?
            {
                return Ok(candidate);
            }
            candidate = format!("{}-{}", base, suffix);
            suffix += 1;
        }
    }

    async fn fetch_concept_state(
        &self,
        concept: &str,
        now: i64,
    ) -> Result<Option<ConceptState>, String> {
        let q = query(
            "MATCH (c:Concept {name: $name})
             RETURN c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at",
        )
        .param("name", concept);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        if let Ok(Some(row)) = result.next().await {
            let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let accessed_at = if accessed_at > 0 { accessed_at } else { now };
            Ok(Some(ConceptState {
                valence,
                arousal_level,
                accessed_at,
            }))
        } else {
            Ok(None)
        }
    }

    async fn update_concept_state(
        &self,
        concept: &str,
        valence: f64,
        arousal_level: Option<f64>,
        accessed_at: Option<i64>,
    ) -> Result<ConceptState, String> {
        let mut qtext = String::from("MATCH (c:Concept {name: $name}) SET c.valence = $valence");
        if arousal_level.is_some() {
            qtext.push_str(", c.arousal_level = $arousal_level");
        }
        if accessed_at.is_some() {
            qtext.push_str(", c.accessed_at = $accessed_at");
        }
        qtext.push_str(" RETURN c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at");
        let mut q = query(qtext.as_str())
            .param("name", concept)
            .param("valence", valence);
        if let Some(level) = arousal_level {
            q = q.param("arousal_level", level);
        }
        if let Some(at) = accessed_at {
            q = q.param("accessed_at", at);
        }
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let row = result
            .next()
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "Failed to update concept: empty result".to_string())?;
        let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
        let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
        let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
        Ok(ConceptState {
            valence,
            arousal_level,
            accessed_at,
        })
    }

    async fn fetch_episode_state(
        &self,
        episode: &str,
        now: i64,
    ) -> Result<Option<EpisodeState>, String> {
        let q = query(
            "MATCH (e:Episode {name: $name})
             RETURN e.valence AS valence, e.arousal_level AS arousal_level, e.accessed_at AS accessed_at",
        )
        .param("name", episode);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        if let Ok(Some(row)) = result.next().await {
            let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let accessed_at = if accessed_at > 0 { accessed_at } else { now };
            Ok(Some(EpisodeState {
                valence,
                arousal_level,
                accessed_at,
            }))
        } else {
            Ok(None)
        }
    }

    async fn update_episode_state(
        &self,
        episode: &str,
        valence: f64,
        arousal_level: Option<f64>,
        accessed_at: Option<i64>,
    ) -> Result<EpisodeState, String> {
        let mut qtext = String::from("MATCH (e:Episode {name: $name}) SET e.valence = $valence");
        if arousal_level.is_some() {
            qtext.push_str(", e.arousal_level = $arousal_level");
        }
        if accessed_at.is_some() {
            qtext.push_str(", e.accessed_at = $accessed_at");
        }
        qtext.push_str(" RETURN e.valence AS valence, e.arousal_level AS arousal_level, e.accessed_at AS accessed_at");
        let mut q = query(qtext.as_str())
            .param("name", episode)
            .param("valence", valence);
        if let Some(level) = arousal_level {
            q = q.param("arousal_level", level);
        }
        if let Some(at) = accessed_at {
            q = q.param("accessed_at", at);
        }
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let row = result
            .next()
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "Failed to update episode: empty result".to_string())?;
        let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
        let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
        let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
        Ok(EpisodeState {
            valence,
            arousal_level,
            accessed_at,
        })
    }

    async fn fetch_arousal_ranked(
        &self,
        exclude: &HashSet<String>,
        limit: usize,
        now: i64,
    ) -> Result<Vec<String>, String> {
        let q = query(
            "MATCH (c:Concept)
             RETURN c.name AS name, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at",
        );
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let mut scored = Vec::<(String, f64)>::new();
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() || exclude.contains(&name) {
                continue;
            }
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            scored.push((name, self.arousal(arousal_level, accessed_at, now)));
        }
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(name, _)| name)
            .collect())
    }

    async fn fetch_relations(&self, concept: &str) -> Result<Vec<RelationEdge>, String> {
        let mut edges = Vec::new();
        let outgoing = query(
            "MATCH (c:Concept {name: $name})-[r:IS_A|PART_OF|EVOKES]->(d:Concept)
             RETURN c.name AS from, d.name AS to, type(r) AS type, coalesce(r.weight, $default_weight) AS weight",
        )
        .param("name", concept)
        .param("default_weight", DEFAULT_RELATION_WEIGHT);
        let mut result = self
            .graph
            .execute(outgoing)
            .await
            .map_err(|err| err.to_string())?;
        while let Ok(Some(row)) = result.next().await {
            edges.push(RelationEdge {
                from: row.get("from").unwrap_or_default(),
                to: row.get("to").unwrap_or_default(),
                relation_type: row.get("type").unwrap_or_default(),
                weight: row.get("weight").unwrap_or(DEFAULT_RELATION_WEIGHT),
            });
        }
        let incoming = query(
            "MATCH (c:Concept {name: $name})<-[r:IS_A|PART_OF|EVOKES]-(d:Concept)
             RETURN d.name AS from, c.name AS to, type(r) AS type, coalesce(r.weight, $default_weight) AS weight",
        )
        .param("name", concept)
        .param("default_weight", DEFAULT_RELATION_WEIGHT);
        let mut result = self
            .graph
            .execute(incoming)
            .await
            .map_err(|err| err.to_string())?;
        while let Ok(Some(row)) = result.next().await {
            edges.push(RelationEdge {
                from: row.get("from").unwrap_or_default(),
                to: row.get("to").unwrap_or_default(),
                relation_type: row.get("type").unwrap_or_default(),
                weight: row.get("weight").unwrap_or(DEFAULT_RELATION_WEIGHT),
            });
        }
        Ok(edges)
    }

    async fn fetch_episodes(&self, concept: &str) -> Result<Vec<EpisodeEntry>, String> {
        let q = query(
            "MATCH (c:Concept {name: $name})-[r:EVOKES]->(e:Episode)
             RETURN e.summary AS summary, e.valence AS valence, coalesce(r.weight, $default_weight) AS weight",
        )
        .param("name", concept)
        .param("default_weight", DEFAULT_RELATION_WEIGHT);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let mut episodes = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            episodes.push(EpisodeEntry {
                summary: row.get("summary").unwrap_or_default(),
                valence: row.get("valence").unwrap_or(DEFAULT_VALENCE),
                weight: row.get("weight").unwrap_or(DEFAULT_RELATION_WEIGHT),
            });
        }
        Ok(episodes)
    }

    async fn get_concept_state_cached(
        &self,
        cache: &mut HashMap<String, ConceptState>,
        concept: &str,
        now: i64,
    ) -> Result<Option<ConceptState>, String> {
        if let Some(state) = cache.get(concept) {
            return Ok(Some(state.clone()));
        }
        let state = self.fetch_concept_state(concept, now).await?;
        if let Some(value) = state.clone() {
            cache.insert(concept.to_string(), value);
        }
        Ok(state)
    }

    async fn maybe_update_arousal(
        &self,
        cache: &mut HashMap<String, ConceptState>,
        concept: &str,
        new_level: f64,
        now: i64,
    ) -> Result<(), String> {
        let current = self
            .get_concept_state_cached(cache, concept, now)
            .await?
            .unwrap_or(ConceptState {
                valence: DEFAULT_VALENCE,
                arousal_level: DEFAULT_AROUSAL_LEVEL,
                accessed_at: now,
            });
        let current_arousal = self.arousal(current.arousal_level, current.accessed_at, now);
        if new_level >= current_arousal {
            let updated = self
                .update_concept_state(concept, current.valence, Some(new_level), Some(now))
                .await?;
            cache.insert(concept.to_string(), updated);
        }
        Ok(())
    }

    async fn activate_related_targets_by_prefix(
        &self,
        concepts: Vec<String>,
        target_prefix: &str,
    ) -> Result<HashMap<String, f64>, String> {
        let now = self.now_ms();
        let mut unique = Vec::<String>::new();
        let mut seen = HashSet::<String>::new();
        for raw in concepts {
            let Some(name) = Self::normalize_non_empty(raw.as_str()) else {
                continue;
            };
            if seen.insert(name.clone()) {
                unique.push(name);
            }
        }
        if unique.is_empty() {
            return Ok(HashMap::new());
        }

        let mut cache = HashMap::<String, ConceptState>::new();
        let mut accumulated = HashMap::<String, f64>::new();
        for concept in unique {
            let Some(source_state) = self
                .get_concept_state_cached(&mut cache, concept.as_str(), now)
                .await?
            else {
                continue;
            };
            let source_arousal =
                self.arousal(source_state.arousal_level, source_state.accessed_at, now);
            if source_arousal <= 0.0 {
                continue;
            }
            let relations = self.fetch_relations(concept.as_str()).await?;
            for edge in relations {
                if edge.from == edge.to {
                    continue;
                }
                let forward = edge.from == concept;
                let target = if forward {
                    edge.to.clone()
                } else {
                    edge.from.clone()
                };
                if !target.starts_with(target_prefix) {
                    continue;
                }
                let direction_penalty = if forward { 1.0 } else { REVERSE_PENALTY };
                let next_level = (source_arousal * edge.weight * direction_penalty).clamp(0.0, 1.0);
                if next_level <= 0.0 {
                    continue;
                }
                let entry = accumulated.entry(target).or_insert(0.0);
                *entry = (*entry + next_level).clamp(0.0, 1.0);
            }
        }
        for (target, level) in &accumulated {
            self.maybe_update_arousal(&mut cache, target.as_str(), *level, now)
                .await?;
        }
        for value in accumulated.values_mut() {
            *value = Self::round_score(*value);
        }
        Ok(accumulated)
    }

    async fn rank_concept_candidates(
        &self,
        candidates: Vec<VectorCandidate>,
        limit: usize,
        now: i64,
    ) -> Result<Vec<String>, String> {
        let mut ranked = Vec::<(String, f64)>::new();
        let mut seen = HashSet::<String>::new();
        for item in candidates {
            let arousal = match self.fetch_concept_state(item.name.as_str(), now).await? {
                Some(state) => self.arousal(state.arousal_level, state.accessed_at, now),
                None => 0.0,
            };
            let final_score = (item.semantic * self.embedding_config.vector_search_semantic_weight)
                + (arousal * self.embedding_config.vector_search_arousal_weight);
            ranked.push((item.name.clone(), final_score));
            seen.insert(item.name);
        }
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let mut concepts = ranked
            .into_iter()
            .take(limit)
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        if concepts.len() < limit {
            let remaining = limit - concepts.len();
            let fallback = self.fetch_arousal_ranked(&seen, remaining, now).await?;
            for name in fallback {
                if concepts.len() >= limit {
                    break;
                }
                concepts.push(name);
            }
        }
        Ok(concepts)
    }
}

#[async_trait]
impl ConceptGraphActivationReader for ActivationConceptGraphStore {
    async fn concept_search(&self, input_text: &str, limit: usize) -> Result<Vec<String>, String> {
        let limit = Self::clamp_limit(limit, 50, 200);
        let now = self.now_ms();
        let candidates = self.vector_search_candidates(input_text, limit).await?;
        self.rank_concept_candidates(candidates, limit, now).await
    }

    async fn concept_search_multimodal(
        &self,
        input: &RouterInput,
        limit: usize,
    ) -> Result<Vec<String>, String> {
        let limit = Self::clamp_limit(limit, 50, 200);
        let now = self.now_ms();
        let candidates = self
            .vector_search_candidates_multimodal(input, limit)
            .await?;
        self.rank_concept_candidates(candidates, limit, now).await
    }

    async fn active_nodes(&self, limit: usize) -> Result<Vec<ActiveGraphNode>, String> {
        let limit = limit.max(1).min(200);
        let now = self.now_ms();
        let mut items = Vec::<(String, f64, i64)>::new();

        let concept_query = query(
            "MATCH (c:Concept)
             RETURN c.name AS name, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at",
        );
        let mut concept_result = self
            .graph
            .execute(concept_query)
            .await
            .map_err(|err| err.to_string())?;
        while let Ok(Some(row)) = concept_result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let arousal = Self::round_score(self.arousal(arousal_level, accessed_at, now));
            if arousal <= 0.0 {
                continue;
            }
            items.push((name, arousal, accessed_at));
        }

        let episode_query = query(
            "MATCH (e:Episode)
             RETURN e.name AS name, e.summary AS summary,
                    e.arousal_level AS arousal_level, e.accessed_at AS accessed_at",
        );
        let mut episode_result = self
            .graph
            .execute(episode_query)
            .await
            .map_err(|err| err.to_string())?;
        while let Ok(Some(row)) = episode_result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let summary: String = row.get("summary").unwrap_or_default();
            let label = if summary.trim().is_empty() {
                name
            } else {
                summary.trim().to_string()
            };
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let arousal = Self::round_score(self.arousal(arousal_level, accessed_at, now));
            if arousal <= 0.0 {
                continue;
            }
            items.push((label, arousal, accessed_at));
        }

        items.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.0.cmp(&b.0))
        });
        items.truncate(limit);

        Ok(items
            .into_iter()
            .map(|(label, arousal, _)| ActiveGraphNode { label, arousal })
            .collect())
    }

    async fn concept_activation(
        &self,
        concepts: &[String],
    ) -> Result<HashMap<String, f64>, String> {
        let now = self.now_ms();
        let mut out = HashMap::<String, f64>::new();
        let mut seen = HashSet::<String>::new();
        for raw in concepts {
            let Some(name) = Self::normalize_non_empty(raw) else {
                continue;
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            let score = match self.fetch_concept_state(name.as_str(), now).await? {
                Some(state) => self.arousal(state.arousal_level, state.accessed_at, now),
                None => 0.0,
            };
            out.insert(name, Self::round_score(score.clamp(0.0, 1.0)));
        }
        Ok(out)
    }

    async fn visible_skills(
        &self,
        threshold: f64,
        limit: usize,
    ) -> Result<Vec<VisibleSkill>, String> {
        let limit = limit.max(1).min(20);
        let threshold = threshold.clamp(0.0, 1.0);
        let now = self.now_ms();
        let q = query(
            "MATCH (c:Concept)
             WHERE (c.kind = 'skill' OR c.name STARTS WITH 'skill:')
               AND coalesce(c.disabled, false) = false
             RETURN c.name AS name,
                    c.summary AS summary,
                    c.body_state_key AS body_state_key,
                    c.arousal_level AS arousal_level,
                    c.accessed_at AS accessed_at",
        );
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let mut items = Vec::<VisibleSkill>::new();
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if !Self::is_skill_name(name.as_str()) {
                continue;
            }
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let score = Self::round_score(self.arousal(arousal_level, accessed_at, now));
            if score < threshold {
                continue;
            }
            let summary: String = row.get("summary").unwrap_or_default();
            let body_state_key: String = row.get("body_state_key").unwrap_or_default();
            items.push(VisibleSkill {
                name: name.clone(),
                summary: Self::skill_summary(name.as_str(), summary.as_str()),
                body_state_key: Self::skill_body_state_key(name.as_str(), body_state_key.as_str()),
                score,
            });
        }
        items.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        items.truncate(limit);
        Ok(items)
    }
}

#[async_trait]
impl ConceptGraphOps for ActivationConceptGraphStore {
    async fn concept_upsert(&self, concept: String) -> Result<Value, String> {
        let concept = Self::normalize_non_empty(concept.as_str())
            .ok_or_else(|| "Error: concept: empty".to_string())?;
        let now = self.now_ms();
        let q = query(
            "MERGE (c:Concept {name: $name})
             ON CREATE SET c.valence = $valence, c.arousal_level = $arousal_level, c.accessed_at = $accessed_at, c._created = true
             ON MATCH SET c._created = false
             WITH c, c._created AS created
             REMOVE c._created
             RETURN created AS created",
        )
        .param("name", concept.as_str())
        .param("valence", DEFAULT_VALENCE)
        .param("arousal_level", INITIAL_AROUSAL_UPSERT)
        .param("accessed_at", now);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let row = result
            .next()
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "Failed to upsert concept: empty result".to_string())?;
        let created: bool = row.get("created").unwrap_or(false);
        self.upsert_concept_embedding(concept.as_str()).await?;
        self.upsert_concept_multimodal_embedding(concept.as_str())
            .await?;
        Ok(json!({
            "concept_id": concept,
            "created": created,
        }))
    }

    async fn skill_index_upsert(
        &self,
        skill_name: String,
        summary: String,
        body_state_key: String,
        enabled: bool,
    ) -> Result<Value, String> {
        let skill_name = Self::normalize_non_empty(skill_name.as_str())
            .ok_or_else(|| "Error: skill_name: empty".to_string())?;
        if !Self::is_skill_name(skill_name.as_str()) {
            return Err("Error: skill_name: must start with skill:".to_string());
        }
        let summary = Self::skill_summary(skill_name.as_str(), summary.as_str());
        let body_state_key =
            Self::skill_body_state_key(skill_name.as_str(), body_state_key.as_str());
        let now = self.now_ms();
        let q = query(
            "MERGE (c:Concept {name: $name})
             ON CREATE SET c.valence = $valence, c.arousal_level = $arousal_level, c.accessed_at = $accessed_at
             SET c.kind = 'skill',
                 c.summary = $summary,
                 c.body_state_key = $body_state_key,
                 c.disabled = $disabled
             RETURN c.name AS name",
        )
        .param("name", skill_name.as_str())
        .param("summary", summary.as_str())
        .param("body_state_key", body_state_key.as_str())
        .param("disabled", !enabled)
        .param("valence", DEFAULT_VALENCE)
        .param("arousal_level", DEFAULT_AROUSAL_LEVEL)
        .param("accessed_at", now);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let _ = result.next().await.map_err(|err| err.to_string())?;
        self.upsert_concept_embedding_for_text(
            skill_name.as_str(),
            format!("{}\n{}", skill_name, summary).as_str(),
        )
        .await?;
        Ok(json!({
            "skill_name": skill_name,
            "summary": summary,
            "body_state_key": body_state_key,
            "enabled": enabled,
        }))
    }

    async fn skill_index_replace_triggers(
        &self,
        skill_name: String,
        trigger_concepts: Vec<String>,
    ) -> Result<Value, String> {
        let skill_name = Self::normalize_non_empty(skill_name.as_str())
            .ok_or_else(|| "Error: skill_name: empty".to_string())?;
        if !Self::is_skill_name(skill_name.as_str()) {
            return Err("Error: skill_name: must start with skill:".to_string());
        }
        let mut deduped = Vec::<String>::new();
        let mut seen = HashSet::<String>::new();
        for item in trigger_concepts {
            let Some(value) = Self::normalize_non_empty(item.as_str()) else {
                continue;
            };
            if seen.insert(value.clone()) {
                deduped.push(value);
            }
        }
        let mut cleanup = self
            .graph
            .execute(
                query(
                    "MATCH (:Concept)-[r:EVOKES]->(c:Concept {name: $name})
                     WHERE r.managed_by = 'state_record_skill_index'
                     DELETE r",
                )
                .param("name", skill_name.as_str()),
            )
            .await
            .map_err(|err| err.to_string())?;
        let _ = cleanup.next().await;
        for trigger in &deduped {
            self.concept_upsert(trigger.clone()).await?;
            let mut result = self
                .graph
                .execute(
                    query(
                        "MATCH (a:Concept {name: $from})
                         MATCH (b:Concept {name: $to})
                         MERGE (a)-[r:EVOKES]->(b)
                         SET r.weight = CASE WHEN r.weight IS NULL THEN $weight ELSE 1 - (1 - r.weight) * (1 - $alpha) END,
                             r.managed_by = 'state_record_skill_index'
                         RETURN type(r) AS type",
                    )
                    .param("from", trigger.as_str())
                    .param("to", skill_name.as_str())
                    .param("weight", DEFAULT_RELATION_WEIGHT)
                    .param("alpha", RELATION_WEIGHT_ALPHA),
                )
                .await
                .map_err(|err| err.to_string())?;
            let _ = result.next().await.map_err(|err| err.to_string())?;
        }
        Ok(json!({
            "skill_name": skill_name,
            "trigger_concepts": deduped,
        }))
    }

    async fn update_affect(&self, target: String, valence_delta: f64) -> Result<Value, String> {
        let target = Self::normalize_non_empty(target.as_str())
            .ok_or_else(|| "Error: target: empty".to_string())?;
        let now = self.now_ms();
        let new_arousal_level = Self::clamp(valence_delta.abs(), 0.0, 1.0);

        if self.episode_exists(target.as_str()).await? {
            let current = self
                .fetch_episode_state(target.as_str(), now)
                .await?
                .unwrap_or(EpisodeState {
                    valence: DEFAULT_VALENCE,
                    arousal_level: DEFAULT_AROUSAL_LEVEL,
                    accessed_at: now,
                });
            let new_valence = Self::clamp(current.valence + valence_delta, -1.0, 1.0);
            let current_arousal = self.arousal(current.arousal_level, current.accessed_at, now);
            let updated = if new_arousal_level >= current_arousal {
                self.update_episode_state(
                    target.as_str(),
                    new_valence,
                    Some(new_arousal_level),
                    Some(now),
                )
                .await?
            } else {
                self.update_episode_state(target.as_str(), new_valence, None, None)
                    .await?
            };
            let arousal = self.arousal(updated.arousal_level, updated.accessed_at, now);
            return Ok(json!({
                "episode_id": target,
                "valence": updated.valence,
                "arousal": arousal,
                "accessed_at": updated.accessed_at,
            }));
        }

        self.ensure_concept(target.as_str(), now, new_arousal_level)
            .await?;
        self.upsert_concept_embedding(target.as_str()).await?;
        self.upsert_concept_multimodal_embedding(target.as_str())
            .await?;
        let current = self
            .fetch_concept_state(target.as_str(), now)
            .await?
            .unwrap_or(ConceptState {
                valence: DEFAULT_VALENCE,
                arousal_level: DEFAULT_AROUSAL_LEVEL,
                accessed_at: now,
            });
        let new_valence = Self::clamp(current.valence + valence_delta, -1.0, 1.0);
        let current_arousal = self.arousal(current.arousal_level, current.accessed_at, now);
        let updated = if new_arousal_level >= current_arousal {
            self.update_concept_state(
                target.as_str(),
                new_valence,
                Some(new_arousal_level),
                Some(now),
            )
            .await?
        } else {
            self.update_concept_state(target.as_str(), new_valence, None, None)
                .await?
        };
        let arousal = self.arousal(updated.arousal_level, updated.accessed_at, now);
        Ok(json!({
            "concept_id": target,
            "valence": updated.valence,
            "arousal": arousal,
            "accessed_at": updated.accessed_at,
        }))
    }

    async fn activate_related_submodules(
        &self,
        concepts: Vec<String>,
    ) -> Result<HashMap<String, f64>, String> {
        self.activate_related_targets_by_prefix(concepts, "submodule:")
            .await
    }

    async fn activate_related_skills(
        &self,
        concepts: Vec<String>,
    ) -> Result<HashMap<String, f64>, String> {
        self.activate_related_targets_by_prefix(concepts, "skill:")
            .await
    }

    async fn dampen_concept_arousal(&self, concept: String, ratio: f64) -> Result<Value, String> {
        let concept = Self::normalize_non_empty(concept.as_str())
            .ok_or_else(|| "Error: concept: empty".to_string())?;
        let now = self.now_ms();
        let ratio = ratio.clamp(0.0, 1.0);
        let Some(current) = self.fetch_concept_state(concept.as_str(), now).await? else {
            return Ok(json!({
                "concept_id": concept,
                "updated": false,
                "reason": "not_found",
            }));
        };
        let current_arousal = self.arousal(current.arousal_level, current.accessed_at, now);
        let next_arousal = (current_arousal * (1.0 - ratio)).clamp(0.0, 1.0);
        let updated = self
            .update_concept_state(
                concept.as_str(),
                current.valence,
                Some(next_arousal),
                Some(now),
            )
            .await?;
        Ok(json!({
            "concept_id": concept,
            "updated": true,
            "ratio": ratio,
            "previous_arousal": Self::round_score(current_arousal),
            "next_arousal": Self::round_score(self.arousal(updated.arousal_level, updated.accessed_at, now)),
            "accessed_at": updated.accessed_at,
        }))
    }

    async fn episode_add(&self, summary: String, concepts: Vec<String>) -> Result<Value, String> {
        let summary = summary.trim().to_string();
        if summary.is_empty() {
            return Err("Error: summary: empty".to_string());
        }
        let concepts = concepts
            .iter()
            .filter_map(|value| Self::normalize_non_empty(value))
            .collect::<Vec<_>>();
        if concepts.is_empty() {
            return Err("Error: concepts: empty".to_string());
        }
        let now = self.now_ms();
        let keyword = concepts[0].clone();
        let date_prefix = Self::local_date_yyyymmdd(now);
        let base_id = format!("{}/{}", date_prefix, keyword);
        let episode_id = self.next_episode_id(base_id.as_str()).await?;
        let q = query(
            "CREATE (e:Episode {name: $name, summary: $summary, valence: $episode_valence, arousal_level: $episode_arousal_level, accessed_at: $accessed_at})
             WITH e
             UNWIND $concepts AS concept
             MERGE (c:Concept {name: concept})
             ON CREATE SET c.valence = $concept_valence, c.arousal_level = $concept_arousal_level, c.accessed_at = $accessed_at
             MERGE (c)-[r:EVOKES]->(e)
             ON CREATE SET r.weight = $relation_weight
             ON MATCH SET r.weight = coalesce(r.weight, $relation_weight)",
        )
        .param("name", episode_id.as_str())
        .param("summary", summary.as_str())
        .param("episode_valence", DEFAULT_VALENCE)
        .param("episode_arousal_level", INITIAL_AROUSAL_UPSERT)
        .param("concepts", concepts.clone())
        .param("concept_valence", DEFAULT_VALENCE)
        .param("concept_arousal_level", INITIAL_AROUSAL_INDIRECT)
        .param("accessed_at", now)
        .param("relation_weight", DEFAULT_RELATION_WEIGHT);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let _ = result.next().await;
        for concept in &concepts {
            self.upsert_concept_embedding(concept.as_str()).await?;
            self.upsert_concept_multimodal_embedding(concept.as_str())
                .await?;
        }
        Ok(json!({
            "episode_id": episode_id,
            "linked_concepts": concepts,
            "valence": DEFAULT_VALENCE,
        }))
    }

    async fn relation_add(
        &self,
        from: String,
        to: String,
        relation_type: String,
    ) -> Result<Value, String> {
        let from = Self::normalize_non_empty(from.as_str())
            .ok_or_else(|| "Error: from: empty".to_string())?;
        let to =
            Self::normalize_non_empty(to.as_str()).ok_or_else(|| "Error: to: empty".to_string())?;
        if from == to {
            return Err("Error: relation: tautology".to_string());
        }
        let relation_type = Self::parse_relation_type(relation_type.as_str())
            .ok_or_else(|| "Error: type: invalid".to_string())?;
        let from_is_episode = self.episode_exists(from.as_str()).await?;
        let to_is_episode = self.episode_exists(to.as_str()).await?;
        if relation_type != RelationType::Evokes && (from_is_episode || to_is_episode) {
            return Err("Error: relation: episode endpoint not allowed".to_string());
        }
        let rel = Self::map_relation_label(&relation_type);
        let now = self.now_ms();
        let qtext = match (from_is_episode, to_is_episode) {
            (false, false) => format!(
                "MERGE (a:Concept {{name: $from}})
                 ON CREATE SET a.valence = $valence, a.arousal_level = $arousal_level, a.accessed_at = $accessed_at
                 MERGE (b:Concept {{name: $to}})
                 ON CREATE SET b.valence = $valence, b.arousal_level = $arousal_level, b.accessed_at = $accessed_at
                 MERGE (a)-[r:{rel}]->(b)
                 SET r.weight = CASE WHEN r.weight IS NULL THEN $weight ELSE 1 - (1 - r.weight) * (1 - $alpha) END
                 RETURN type(r) AS type",
            ),
            (false, true) => format!(
                "MATCH (b:Episode {{name: $to}})
                 MERGE (a:Concept {{name: $from}})
                 ON CREATE SET a.valence = $valence, a.arousal_level = $arousal_level, a.accessed_at = $accessed_at
                 MERGE (a)-[r:{rel}]->(b)
                 SET r.weight = CASE WHEN r.weight IS NULL THEN $weight ELSE 1 - (1 - r.weight) * (1 - $alpha) END
                 RETURN type(r) AS type",
            ),
            (true, false) => format!(
                "MATCH (a:Episode {{name: $from}})
                 MERGE (b:Concept {{name: $to}})
                 ON CREATE SET b.valence = $valence, b.arousal_level = $arousal_level, b.accessed_at = $accessed_at
                 MERGE (a)-[r:{rel}]->(b)
                 SET r.weight = CASE WHEN r.weight IS NULL THEN $weight ELSE 1 - (1 - r.weight) * (1 - $alpha) END
                 RETURN type(r) AS type",
            ),
            (true, true) => format!(
                "MATCH (a:Episode {{name: $from}})
                 MATCH (b:Episode {{name: $to}})
                 MERGE (a)-[r:{rel}]->(b)
                 SET r.weight = CASE WHEN r.weight IS NULL THEN $weight ELSE 1 - (1 - r.weight) * (1 - $alpha) END
                 RETURN type(r) AS type",
            ),
        };
        let q = query(qtext.as_str())
            .param("from", from.as_str())
            .param("to", to.as_str())
            .param("valence", DEFAULT_VALENCE)
            .param("arousal_level", INITIAL_AROUSAL_INDIRECT)
            .param("accessed_at", now)
            .param("weight", DEFAULT_RELATION_WEIGHT)
            .param("alpha", RELATION_WEIGHT_ALPHA);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let _ = result.next().await;
        if !from_is_episode {
            self.upsert_concept_embedding(from.as_str()).await?;
            self.upsert_concept_multimodal_embedding(from.as_str())
                .await?;
        }
        if !to_is_episode {
            self.upsert_concept_embedding(to.as_str()).await?;
            self.upsert_concept_multimodal_embedding(to.as_str())
                .await?;
        }
        Ok(json!({
            "from": from,
            "to": to,
            "type": match relation_type {
                RelationType::IsA => "is-a",
                RelationType::PartOf => "part-of",
                RelationType::Evokes => "evokes",
            },
        }))
    }

    async fn recall_query(&self, seeds: Vec<String>, max_hop: u32, dry_run: bool) -> Result<Value, String> {
        if max_hop == 0 {
            return Ok(json!({ "propositions": [] }));
        }
        let seeds = seeds
            .iter()
            .filter_map(|value| Self::normalize_non_empty(value))
            .collect::<Vec<_>>();
        if seeds.is_empty() {
            return Ok(json!({ "propositions": [] }));
        }
        let now = self.now_ms();
        let mut cache: HashMap<String, ConceptState> = HashMap::new();
        let mut visited: HashMap<String, u32> = HashMap::new();
        let mut queue: VecDeque<(String, u32)> = VecDeque::new();
        let mut propositions: HashMap<String, Proposition> = HashMap::new();
        for seed in seeds {
            if !visited.contains_key(seed.as_str()) {
                visited.insert(seed.clone(), 0);
                queue.push_back((seed, 0));
            }
        }
        while let Some((concept, hop)) = queue.pop_front() {
            if hop >= max_hop {
                continue;
            }
            let next_hop = hop + 1;
            let relations = self.fetch_relations(concept.as_str()).await?;
            for edge in relations {
                if edge.from == edge.to {
                    continue;
                }
                let forward = edge.from == concept;
                let target = if forward {
                    edge.to.clone()
                } else {
                    edge.from.clone()
                };
                if let Some(target_state) = self
                    .get_concept_state_cached(&mut cache, target.as_str(), now)
                    .await?
                {
                    let hop_decay = Self::hop_decay(next_hop);
                    let direction_penalty = if forward { 1.0 } else { REVERSE_PENALTY };
                    let arousal =
                        self.arousal(target_state.arousal_level, target_state.accessed_at, now);
                    let score = arousal * hop_decay * direction_penalty * edge.weight;
                    let text = format!(
                        "{} {} {}",
                        edge.from,
                        Self::render_relation_type(edge.relation_type.as_str()),
                        edge.to
                    );
                    let proposition = Proposition {
                        text: text.clone(),
                        score,
                        valence: Some(target_state.valence),
                    };
                    let entry = propositions.entry(text).or_insert(proposition);
                    if score > entry.score {
                        entry.score = score;
                        entry.valence = Some(target_state.valence);
                    }
                    if visited
                        .get(target.as_str())
                        .map(|existing| next_hop < *existing)
                        .unwrap_or(true)
                    {
                        visited.insert(target.clone(), next_hop);
                        queue.push_back((target.clone(), next_hop));
                    }
                    // Keep submodule trigger nodes from self-sustaining activation across turns.
                    if !dry_run && !target.starts_with("submodule:") {
                        self.maybe_update_arousal(&mut cache, target.as_str(), hop_decay, now)
                            .await?;
                    }
                }
            }
            let episodes = self.fetch_episodes(concept.as_str()).await?;
            if !episodes.is_empty() {
                if let Some(concept_state) = self
                    .get_concept_state_cached(&mut cache, concept.as_str(), now)
                    .await?
                {
                    let hop_decay = Self::hop_decay(next_hop);
                    let arousal =
                        self.arousal(concept_state.arousal_level, concept_state.accessed_at, now);
                    for episode in episodes {
                        let score = arousal * hop_decay * episode.weight;
                        let text = format!("{} evokes {}", concept, episode.summary);
                        let proposition = Proposition {
                            text: text.clone(),
                            score,
                            valence: Some(episode.valence),
                        };
                        let entry = propositions.entry(text).or_insert(proposition);
                        if score > entry.score {
                            entry.score = score;
                            entry.valence = Some(concept_state.valence);
                        }
                    }
                }
            }
        }
        let mut items = propositions.into_values().collect::<Vec<_>>();
        items.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(json!({
            "propositions": items
                .into_iter()
                .map(|mut item| {
                    item.score = Self::round_score(item.score);
                    json!({
                        "text": item.text,
                        "score": item.score,
                        "valence": item.valence,
                    })
                })
                .collect::<Vec<_>>(),
        }))
    }

}

#[async_trait]
impl ConceptGraphDebugReader for ActivationConceptGraphStore {
    async fn debug_health(&self) -> Result<Value, String> {
        let q = query("RETURN 1 AS ok");
        match self.graph.execute(q).await {
            Ok(mut result) => {
                let _ = result.next().await;
                Ok(json!({
                    "connected": true,
                    "arousal_tau_ms": self.arousal_tau_ms,
                    "checked_at_ms": self.now_ms(),
                }))
            }
            Err(err) => Ok(json!({
                "connected": false,
                "error": err.to_string(),
                "checked_at_ms": self.now_ms(),
            })),
        }
    }

    async fn debug_counts(&self) -> Result<Value, String> {
        let mut concept_count = 0_i64;
        let mut episode_count = 0_i64;
        let mut relation_count = 0_i64;

        let q_concepts = query("MATCH (c:Concept) RETURN count(c) AS count");
        let mut concept_result = self
            .graph
            .execute(q_concepts)
            .await
            .map_err(|err| err.to_string())?;
        if let Ok(Some(row)) = concept_result.next().await {
            concept_count = row.get("count").unwrap_or(0);
        }

        let q_episodes = query("MATCH (e:Episode) RETURN count(e) AS count");
        let mut episode_result = self
            .graph
            .execute(q_episodes)
            .await
            .map_err(|err| err.to_string())?;
        if let Ok(Some(row)) = episode_result.next().await {
            episode_count = row.get("count").unwrap_or(0);
        }

        let q_relations = query("MATCH ()-[r:IS_A|PART_OF|EVOKES]->() RETURN count(r) AS count");
        let mut relation_result = self
            .graph
            .execute(q_relations)
            .await
            .map_err(|err| err.to_string())?;
        if let Ok(Some(row)) = relation_result.next().await {
            relation_count = row.get("count").unwrap_or(0);
        }

        Ok(json!({
            "concept_count": concept_count,
            "episode_count": episode_count,
            "relation_count": relation_count,
        }))
    }

    async fn debug_concept_search(
        &self,
        query_text: Option<String>,
        limit: usize,
    ) -> Result<Vec<Value>, String> {
        let query_text = query_text.and_then(|value| Self::normalize_non_empty(&value));
        let query_lower = query_text.as_deref().map(|value| value.to_lowercase());
        let limit = Self::clamp_limit(limit, 50, 200);
        let now = self.now_ms();
        let q = query(
            "MATCH (c:Concept)
             RETURN c.name AS name,
                    c.summary AS summary,
                    c.body_state_key AS body_state_key,
                    c.disabled AS disabled,
                    c.valence AS valence,
                    c.arousal_level AS arousal_level,
                    c.accessed_at AS accessed_at",
        );
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let mut items = Vec::<Value>::new();
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            if let Some(expected) = query_lower.as_deref() {
                if !name.to_lowercase().contains(expected) {
                    continue;
                }
            }
            let summary_raw: String = row.get("summary").unwrap_or_default();
            let body_state_key_raw: String = row.get("body_state_key").unwrap_or_default();
            let disabled: bool = row.get("disabled").unwrap_or(false);
            let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let arousal = Self::round_score(self.arousal(arousal_level, accessed_at, now));
            let is_skill = Self::is_skill_name(name.as_str());
            items.push(json!({
                "name": name,
                "kind": if is_skill { "skill" } else { "concept" },
                "summary": if is_skill { Self::skill_summary(name.as_str(), summary_raw.as_str()) } else { String::new() },
                "body_state_key": if is_skill { Self::skill_body_state_key(name.as_str(), body_state_key_raw.as_str()) } else { String::new() },
                "disabled": disabled,
                "valence": valence,
                "arousal": arousal,
                "arousal_level": arousal_level,
                "accessed_at": accessed_at,
            }));
        }
        items.sort_by(|a, b| {
            let a_arousal = a
                .get("arousal")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let b_arousal = b
                .get("arousal")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let a_accessed_at = a
                .get("accessed_at")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            let b_accessed_at = b
                .get("accessed_at")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            b_arousal
                .partial_cmp(&a_arousal)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b_accessed_at.cmp(&a_accessed_at))
                .then_with(|| {
                    let a_name = a.get("name").and_then(|value| value.as_str()).unwrap_or("");
                    let b_name = b.get("name").and_then(|value| value.as_str()).unwrap_or("");
                    a_name.cmp(b_name)
                })
        });
        items.truncate(limit);
        Ok(items)
    }

    async fn debug_concept_detail(&self, concept: String) -> Result<Option<Value>, String> {
        let concept = match Self::normalize_non_empty(&concept) {
            Some(value) => value,
            None => return Ok(None),
        };
        let now = self.now_ms();
        let Some(state) = self.fetch_concept_state(concept.as_str(), now).await? else {
            return Ok(None);
        };
        let mut meta_result = self
            .graph
            .execute(
                query(
                    "MATCH (c:Concept {name: $name})
                     RETURN c.summary AS summary, c.body_state_key AS body_state_key, c.disabled AS disabled",
                )
                .param("name", concept.as_str()),
            )
            .await
            .map_err(|err| err.to_string())?;
        let mut summary_raw = String::new();
        let mut body_state_key_raw = String::new();
        let mut disabled = false;
        if let Ok(Some(row)) = meta_result.next().await {
            summary_raw = row.get("summary").unwrap_or_default();
            body_state_key_raw = row.get("body_state_key").unwrap_or_default();
            disabled = row.get("disabled").unwrap_or(false);
        }
        let relations = self
            .fetch_relations(concept.as_str())
            .await?
            .into_iter()
            .map(|edge| {
                let direction = if edge.from == concept {
                    "outgoing"
                } else {
                    "incoming"
                };
                json!({
                    "direction": direction,
                    "from": edge.from,
                    "to": edge.to,
                    "type": Self::render_relation_type(edge.relation_type.as_str()),
                    "weight": edge.weight,
                })
            })
            .collect::<Vec<_>>();
        let episodes = self
            .fetch_episodes(concept.as_str())
            .await?
            .into_iter()
            .map(|episode| {
                json!({
                    "summary": episode.summary,
                    "valence": episode.valence,
                    "weight": episode.weight,
                })
            })
            .collect::<Vec<_>>();
        let is_skill = Self::is_skill_name(concept.as_str());
        Ok(Some(json!({
            "name": concept,
            "kind": if is_skill { "skill" } else { "concept" },
            "summary": if is_skill { Self::skill_summary(concept.as_str(), summary_raw.as_str()) } else { String::new() },
            "body_state_key": if is_skill { Self::skill_body_state_key(concept.as_str(), body_state_key_raw.as_str()) } else { String::new() },
            "disabled": disabled,
            "valence": state.valence,
            "arousal": Self::round_score(self.arousal(state.arousal_level, state.accessed_at, now)),
            "arousal_level": state.arousal_level,
            "accessed_at": state.accessed_at,
            "relations": relations,
            "episodes": episodes,
        })))
    }

    async fn debug_episode_search(
        &self,
        query_text: Option<String>,
        limit: usize,
    ) -> Result<Vec<Value>, String> {
        let query_text = query_text.and_then(|value| Self::normalize_non_empty(&value));
        let query_lower = query_text.as_deref().map(|value| value.to_lowercase());
        let limit = Self::clamp_limit(limit, 50, 200);
        let now = self.now_ms();
        let q = query(
            "MATCH (e:Episode)
             RETURN e.name AS name, e.summary AS summary, e.valence AS valence,
                    e.arousal_level AS arousal_level, e.accessed_at AS accessed_at
             ORDER BY e.name",
        );
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let mut items = Vec::<Value>::new();
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let summary: String = row.get("summary").unwrap_or_default();
            if let Some(expected) = query_lower.as_deref() {
                let hay = format!("{} {}", name.to_lowercase(), summary.to_lowercase());
                if !hay.contains(expected) {
                    continue;
                }
            }
            let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let q_linked = query(
                "MATCH (c:Concept)-[:EVOKES]->(e:Episode {name: $name}) RETURN count(c) AS count",
            )
            .param("name", name.as_str());
            let mut linked_result = self
                .graph
                .execute(q_linked)
                .await
                .map_err(|err| err.to_string())?;
            let linked_concepts = if let Ok(Some(linked_row)) = linked_result.next().await {
                linked_row.get("count").unwrap_or(0)
            } else {
                0
            };
            let arousal = Self::round_score(self.arousal(arousal_level, accessed_at, now));
            items.push(json!({
                "name": name,
                "summary": summary,
                "valence": valence,
                "arousal": arousal,
                "arousal_level": arousal_level,
                "accessed_at": accessed_at,
                "linked_concepts": linked_concepts,
            }));
        }
        items.sort_by(|a, b| {
            let a_arousal = a
                .get("arousal")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let b_arousal = b
                .get("arousal")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let a_accessed_at = a
                .get("accessed_at")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            let b_accessed_at = b
                .get("accessed_at")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            b_arousal
                .partial_cmp(&a_arousal)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b_accessed_at.cmp(&a_accessed_at))
                .then_with(|| {
                    let a_name = a.get("name").and_then(|value| value.as_str()).unwrap_or("");
                    let b_name = b.get("name").and_then(|value| value.as_str()).unwrap_or("");
                    a_name.cmp(b_name)
                })
        });
        items.truncate(limit);
        Ok(items)
    }

    async fn debug_episode_detail(&self, episode: String) -> Result<Option<Value>, String> {
        let episode = match Self::normalize_non_empty(&episode) {
            Some(value) => value,
            None => return Ok(None),
        };
        let now = self.now_ms();
        let Some(state) = self.fetch_episode_state(episode.as_str(), now).await? else {
            return Ok(None);
        };
        let q = query(
            "MATCH (e:Episode {name: $name})
             OPTIONAL MATCH (c:Concept)-[r:EVOKES]->(e)
             RETURN c.name AS concept, coalesce(r.weight, $default_weight) AS weight
             ORDER BY concept",
        )
        .param("name", episode.as_str())
        .param("default_weight", DEFAULT_RELATION_WEIGHT);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let mut linked = Vec::<Value>::new();
        while let Ok(Some(row)) = result.next().await {
            let concept: String = row.get("concept").unwrap_or_default();
            if concept.is_empty() {
                continue;
            }
            let weight: f64 = row.get("weight").unwrap_or(DEFAULT_RELATION_WEIGHT);
            linked.push(json!({
                "concept": concept,
                "weight": weight,
            }));
        }
        Ok(Some(json!({
            "name": episode,
            "valence": state.valence,
            "arousal": Self::round_score(self.arousal(state.arousal_level, state.accessed_at, now)),
            "arousal_level": state.arousal_level,
            "accessed_at": state.accessed_at,
            "linked_concepts": linked,
        })))
    }

    async fn debug_relation_search(
        &self,
        query_text: Option<String>,
        limit: usize,
    ) -> Result<Vec<Value>, String> {
        let query_text = query_text.and_then(|value| Self::normalize_non_empty(&value));
        let query_lower = query_text.as_deref().map(|value| value.to_lowercase());
        let limit = Self::clamp_limit(limit, 50, 200);
        let q = query(
            "MATCH (a)-[r:IS_A|PART_OF|EVOKES]->(b)
             RETURN a.name AS from, b.name AS to, type(r) AS type, coalesce(r.weight, $default_weight) AS weight
             ORDER BY weight DESC, from, to",
        )
        .param("default_weight", DEFAULT_RELATION_WEIGHT);
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let mut items = Vec::<Value>::new();
        while let Ok(Some(row)) = result.next().await {
            let from: String = row.get("from").unwrap_or_default();
            let to: String = row.get("to").unwrap_or_default();
            if from.is_empty() || to.is_empty() {
                continue;
            }
            let relation_type_raw: String = row.get("type").unwrap_or_default();
            let relation_type = Self::render_relation_type(relation_type_raw.as_str()).to_string();
            if let Some(expected) = query_lower.as_deref() {
                let hay = format!(
                    "{} {} {}",
                    from.to_lowercase(),
                    relation_type.to_lowercase(),
                    to.to_lowercase()
                );
                if !hay.contains(expected) {
                    continue;
                }
            }
            let weight: f64 = row.get("weight").unwrap_or(DEFAULT_RELATION_WEIGHT);
            items.push(json!({
                "from": from,
                "to": to,
                "type": relation_type,
                "weight": weight,
            }));
            if items.len() >= limit {
                break;
            }
        }
        Ok(items)
    }
}

#[async_trait]
impl ConversationRecallStore for ActivationConceptGraphStore {
    async fn upsert_event_projection(&self, event: &crate::event::Event) -> Result<(), String> {
        let Some(text) = conversation_recall_text(event) else {
            return Ok(());
        };
        let embedding = self.embedding.encode(text.as_str())?;
        let embedding = Self::to_embedding_property_vector(&embedding);
        let mut stream = self
            .graph
            .execute(
                query(
                    "MERGE (e:ConversationEvent {event_id: $event_id})
                     SET e.ts = $ts, e.source = $source, e.embedding = $embedding
                     RETURN e.event_id AS event_id",
                )
                .param("event_id", event.event_id.as_str())
                .param("ts", event.ts.as_str())
                .param("source", event.source.as_str())
                .param("embedding", embedding),
            )
            .await
            .map_err(|err| {
                format!(
                    "upsert conversation event projection failed for {}: {}",
                    event.event_id, err
                )
            })?;
        let _ = stream.next().await;
        Ok(())
    }

    async fn search_event_projections(
        &self,
        input_text: &str,
        limit: usize,
    ) -> Result<Vec<ConversationRecallCandidate>, String> {
        let query_text = input_text.trim();
        if query_text.is_empty() {
            return Ok(Vec::new());
        }
        let query_embedding = self.embedding.encode(query_text)?;
        let query_embedding = Self::to_embedding_property_vector(&query_embedding);
        let cypher = format!(
            "CALL vector_search.search(\"{}\", $limit, $embedding) YIELD node, similarity
             RETURN node.event_id AS event_id, similarity
             ORDER BY similarity DESC",
            DEFAULT_CONVERSATION_VECTOR_INDEX_NAME
        );
        let mut result = self
            .graph
            .execute(
                query(cypher.as_str())
                    .param("limit", limit.max(1) as i64)
                    .param("embedding", query_embedding),
            )
            .await
            .map_err(|err| format!("conversation vector_search.search failed: {}", err))?;
        let mut out = Vec::<ConversationRecallCandidate>::new();
        let mut seen = HashSet::<String>::new();
        while let Ok(Some(row)) = result.next().await {
            let event_id: String = row.get("event_id").unwrap_or_default();
            if event_id.is_empty() || !seen.insert(event_id.clone()) {
                continue;
            }
            let semantic_similarity = row.get::<f64>("similarity").unwrap_or(0.0).clamp(0.0, 1.0);
            out.push(ConversationRecallCandidate {
                event_id,
                semantic_similarity,
            });
            if out.len() >= limit.max(1) {
                break;
            }
        }
        Ok(out)
    }
}
