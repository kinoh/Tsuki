use neo4rs::{Graph, query};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const DEFAULT_VALENCE: f64 = 0.0;
const DEFAULT_AROUSAL_LEVEL: f64 = 0.0;
const INITIAL_AROUSAL_UPSERT: f64 = 0.5;
const INITIAL_AROUSAL_INDIRECT: f64 = 0.25;
const DEFAULT_ACCESSED_AT: i64 = 0;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConceptUpsertRequest {
    pub concept: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConceptUpdateAffectRequest {
    pub concept: String,
    pub valence_delta: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EpisodeAddRequest {
    pub summary: String,
    pub concepts: Vec<String>,
    pub valence: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RelationAddRequest {
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub relation_type: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecallQueryRequest {
    pub seeds: Vec<String>,
    pub max_hop: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Proposition {
    pub text: String,
    pub score: f64,
    pub valence: Option<f64>,
}

#[derive(Debug, Clone)]
struct ConceptState {
    valence: f64,
    arousal_level: f64,
    accessed_at: i64,
}

#[derive(Debug, Clone)]
struct RelationEdge {
    from: String,
    to: String,
    relation_type: String,
}

#[derive(Debug, Clone)]
struct EpisodeEntry {
    id: String,
    summary: String,
    valence: f64,
}

#[derive(Clone)]
pub struct ConceptGraphService {
    tool_router: ToolRouter<Self>,
    graph: Arc<Graph>,
    arousal_tau_ms: f64,
}

impl ConceptGraphService {
    pub async fn connect(
        uri: String,
        user: String,
        password: String,
        arousal_tau_ms: f64,
    ) -> Result<Self, Box<dyn Error>> {
        let graph = Graph::new(uri, user, password)?;
        let tau = if arousal_tau_ms > 0.0 { arousal_tau_ms } else { 1.0 };
        Ok(Self {
            tool_router: Self::tool_router(),
            graph: Arc::new(graph),
            arousal_tau_ms: tau,
        })
    }

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0)
    }

    fn clamp(val: f64, min: f64, max: f64) -> f64 {
        val.max(min).min(max)
    }

    fn hop_decay(hop: u32, forward: bool) -> f64 {
        if forward {
            0.5_f64.powi((hop.saturating_sub(1)) as i32)
        } else {
            0.5_f64.powi(hop as i32)
        }
    }

    fn arousal(&self, level: f64, accessed_at: i64, now: i64) -> f64 {
        let delta_ms = (now - accessed_at).max(0) as f64;
        let decay = (-delta_ms / self.arousal_tau_ms).exp();
        level * decay
    }

    fn invalid_params(message: &str, detail: serde_json::Value) -> ErrorData {
        ErrorData::invalid_params(message.to_string(), Some(detail))
    }

    fn internal_error(message: &str, err: impl ToString) -> ErrorData {
        ErrorData::internal_error(message.to_string(), Some(json!({"reason": err.to_string()})))
    }

    fn normalize_concept(&self, concept: &str) -> Option<String> {
        let trimmed = concept.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn map_relation_type(&self, relation_type: &str) -> Result<&'static str, ErrorData> {
        match relation_type {
            "is-a" => Ok("IS_A"),
            "part-of" => Ok("PART_OF"),
            "evokes" => Ok("EVOKES"),
            _ => Err(Self::invalid_params(
                "Error: type: invalid",
                json!({"type": relation_type}),
            )),
        }
    }

    fn render_relation_type(&self, relation_type: &str) -> &'static str {
        match relation_type {
            "IS_A" => "is-a",
            "PART_OF" => "part-of",
            "EVOKES" => "evokes",
            _ => "evokes",
        }
    }

    async fn ensure_concept(
        &self,
        concept: &str,
        now: i64,
        initial_arousal_level: f64,
    ) -> Result<(), ErrorData> {
        let query = query(
            "MERGE (c:Concept {name: $name})\n\
             ON CREATE SET c.valence = $valence, c.arousal_level = $arousal_level, c.accessed_at = $accessed_at\n\
             RETURN c.name AS name",
        )
        .param("name", concept)
        .param("valence", DEFAULT_VALENCE)
        .param("arousal_level", initial_arousal_level)
        .param("accessed_at", now);

        let mut result = self.graph.execute(query).await.map_err(|e| {
            Self::internal_error("Failed to ensure concept", e)
        })?;

        if let Err(e) = result.next().await {
            return Err(Self::internal_error("Failed to ensure concept", e));
        }

        Ok(())
    }

    async fn concept_exists(&self, concept: &str) -> Result<bool, ErrorData> {
        let query = query(
            "MATCH (c:Concept {name: $name}) RETURN count(c) AS count",
        )
        .param("name", concept);

        let mut result = self.graph.execute(query).await.map_err(|e| {
            Self::internal_error("Failed to check concept", e)
        })?;

        if let Ok(Some(row)) = result.next().await {
            let count: i64 = row.get("count").unwrap_or(0);
            Ok(count > 0)
        } else {
            Ok(false)
        }
    }

    async fn fetch_concept_state(
        &self,
        concept: &str,
        now: i64,
    ) -> Result<Option<ConceptState>, ErrorData> {
        let query = query(
            "MATCH (c:Concept {name: $name})\n\
             RETURN c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at",
        )
        .param("name", concept);

        let mut result = self.graph.execute(query).await.map_err(|e| {
            Self::internal_error("Failed to load concept", e)
        })?;

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
    ) -> Result<ConceptState, ErrorData> {
        let mut query_text = String::from("MATCH (c:Concept {name: $name}) SET c.valence = $valence");
        if arousal_level.is_some() {
            query_text.push_str(", c.arousal_level = $arousal_level");
        }
        if accessed_at.is_some() {
            query_text.push_str(", c.accessed_at = $accessed_at");
        }
        query_text.push_str(" RETURN c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at");

        let mut query = query(query_text.as_str())
            .param("name", concept)
            .param("valence", valence);

        if let Some(level) = arousal_level {
            query = query.param("arousal_level", level);
        }
        if let Some(accessed_at) = accessed_at {
            query = query.param("accessed_at", accessed_at);
        }

        let mut result = self.graph.execute(query).await.map_err(|e| {
            Self::internal_error("Failed to update concept", e)
        })?;

        if let Ok(Some(row)) = result.next().await {
            let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);

            Ok(ConceptState {
                valence,
                arousal_level,
                accessed_at,
            })
        } else {
            Err(Self::internal_error(
                "Failed to update concept",
                "empty result",
            ))
        }
    }

    async fn fetch_relations(&self, concept: &str) -> Result<Vec<RelationEdge>, ErrorData> {
        let mut edges = Vec::new();

        let outgoing = query(
            "MATCH (c:Concept {name: $name})-[r:IS_A|PART_OF|EVOKES]->(d:Concept)\n\
             RETURN c.name AS from, d.name AS to, type(r) AS type",
        )
        .param("name", concept);

        let mut result = self.graph.execute(outgoing).await.map_err(|e| {
            Self::internal_error("Failed to load relations", e)
        })?;

        while let Ok(Some(row)) = result.next().await {
            let from: String = row.get("from").unwrap_or_default();
            let to: String = row.get("to").unwrap_or_default();
            let relation_type: String = row.get("type").unwrap_or_default();
            edges.push(RelationEdge {
                from,
                to,
                relation_type,
            });
        }

        let incoming = query(
            "MATCH (c:Concept {name: $name})<-[r:IS_A|PART_OF|EVOKES]-(d:Concept)\n\
             RETURN d.name AS from, c.name AS to, type(r) AS type",
        )
        .param("name", concept);

        let mut result = self.graph.execute(incoming).await.map_err(|e| {
            Self::internal_error("Failed to load relations", e)
        })?;

        while let Ok(Some(row)) = result.next().await {
            let from: String = row.get("from").unwrap_or_default();
            let to: String = row.get("to").unwrap_or_default();
            let relation_type: String = row.get("type").unwrap_or_default();
            edges.push(RelationEdge {
                from,
                to,
                relation_type,
            });
        }

        Ok(edges)
    }

    async fn fetch_episodes(&self, concept: &str) -> Result<Vec<EpisodeEntry>, ErrorData> {
        let query = query(
            "MATCH (c:Concept {name: $name})-[:EVOKES]->(e:Episode)\n\
             RETURN e.id AS id, e.summary AS summary, e.valence AS valence",
        )
        .param("name", concept);

        let mut result = self.graph.execute(query).await.map_err(|e| {
            Self::internal_error("Failed to load episodes", e)
        })?;

        let mut episodes = Vec::new();
        while let Ok(Some(row)) = result.next().await {
            let id: String = row.get("id").unwrap_or_default();
            let summary: String = row.get("summary").unwrap_or_default();
            let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
            episodes.push(EpisodeEntry {
                id,
                summary,
                valence,
            });
        }

        Ok(episodes)
    }

    async fn get_concept_state_cached(
        &self,
        cache: &mut HashMap<String, ConceptState>,
        concept: &str,
        now: i64,
    ) -> Result<Option<ConceptState>, ErrorData> {
        if let Some(state) = cache.get(concept) {
            return Ok(Some(state.clone()));
        }

        if let Some(state) = self.fetch_concept_state(concept, now).await? {
            cache.insert(concept.to_string(), state.clone());
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }

    async fn maybe_update_arousal(
        &self,
        cache: &mut HashMap<String, ConceptState>,
        concept: &str,
        new_level: f64,
        now: i64,
    ) -> Result<(), ErrorData> {
        let current = self
            .get_concept_state_cached(cache, concept, now)
            .await?
            .unwrap_or(ConceptState {
                valence: DEFAULT_VALENCE,
                arousal_level: DEFAULT_AROUSAL_LEVEL,
                accessed_at: now,
            });

        let current_arousal = self.arousal(current.arousal_level, current.accessed_at, now);
        let new_arousal = new_level;

        if new_arousal >= current_arousal {
            let updated = self
                .update_concept_state(concept, current.valence, Some(new_level), Some(now))
                .await?;
            cache.insert(concept.to_string(), updated);
        }

        Ok(())
    }
}

#[tool_router]
impl ConceptGraphService {
    #[tool(description = "Creates the concept if missing. Uses the concept string as-is.")]
    pub async fn concept_upsert(
        &self,
        params: Parameters<ConceptUpsertRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        let concept = self
            .normalize_concept(&request.concept)
            .ok_or_else(|| Self::invalid_params("Error: concept: empty", json!({})))?;

        let exists = self.concept_exists(&concept).await?;
        let now = Self::now_ms();

        if !exists {
            self.ensure_concept(&concept, now, INITIAL_AROUSAL_UPSERT)
                .await?;
        }

        let result = json!({
            "concept_id": concept,
            "created": !exists,
        });

        Ok(CallToolResult {
            content: vec![Content::text(result.to_string())],
            structured_content: Some(result),
            is_error: Some(false),
            meta: None,
        })
    }

    #[tool(description = "Adjusts valence by delta and conditionally updates arousal level.")]
    pub async fn concept_update_affect(
        &self,
        params: Parameters<ConceptUpdateAffectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        let concept = self
            .normalize_concept(&request.concept)
            .ok_or_else(|| Self::invalid_params("Error: concept: empty", json!({})))?;

        let now = Self::now_ms();
        let new_arousal_level = Self::clamp(request.valence_delta.abs(), 0.0, 1.0);

        if !self.concept_exists(&concept).await? {
            self.ensure_concept(&concept, now, new_arousal_level)
                .await?;
        }

        let current = self
            .fetch_concept_state(&concept, now)
            .await?
            .unwrap_or(ConceptState {
                valence: DEFAULT_VALENCE,
                arousal_level: DEFAULT_AROUSAL_LEVEL,
                accessed_at: now,
            });

        let new_valence = Self::clamp(current.valence + request.valence_delta, -1.0, 1.0);
        let current_arousal = self.arousal(current.arousal_level, current.accessed_at, now);

        let update_arousal = new_arousal_level >= current_arousal;
        let updated = if update_arousal {
            self.update_concept_state(&concept, new_valence, Some(new_arousal_level), Some(now))
                .await?
        } else {
            self.update_concept_state(&concept, new_valence, None, None).await?
        };

        let arousal = self.arousal(updated.arousal_level, updated.accessed_at, now);

        let result = json!({
            "concept_id": concept,
            "valence": updated.valence,
            "arousal": arousal,
            "accessed_at": updated.accessed_at,
        });

        Ok(CallToolResult {
            content: vec![Content::text(result.to_string())],
            structured_content: Some(result),
            is_error: Some(false),
            meta: None,
        })
    }

    #[tool(description = "Adds an episode summary and links it to concepts.")]
    pub async fn episode_add(
        &self,
        params: Parameters<EpisodeAddRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        let summary = request.summary.trim();
        if summary.is_empty() {
            return Err(Self::invalid_params("Error: summary: empty", json!({}))); 
        }

        let mut concepts = Vec::new();
        for concept in &request.concepts {
            if let Some(normalized) = self.normalize_concept(concept) {
                concepts.push(normalized);
            }
        }

        if concepts.is_empty() {
            return Err(Self::invalid_params("Error: concepts: empty", json!({}))); 
        }

        let now = Self::now_ms();
        let episode_id = Uuid::new_v4().to_string();
        let episode_valence = Self::clamp(request.valence, -1.0, 1.0);

        let query = query(
            "CREATE (e:Episode {id: $id, summary: $summary, valence: $episode_valence})\n\
             WITH e\n\
             UNWIND $concepts AS concept\n\
             MERGE (c:Concept {name: concept})\n\
             ON CREATE SET c.valence = $concept_valence, c.arousal_level = $concept_arousal_level, c.accessed_at = $accessed_at\n\
             MERGE (c)-[:EVOKES]->(e)",
        )
        .param("id", episode_id.as_str())
        .param("summary", summary)
        .param("episode_valence", episode_valence)
        .param("concepts", concepts.clone())
        .param("concept_valence", DEFAULT_VALENCE)
        .param("concept_arousal_level", INITIAL_AROUSAL_INDIRECT)
        .param("accessed_at", now);

        let mut result = self.graph.execute(query).await.map_err(|e| {
            Self::internal_error("Failed to add episode", e)
        })?;

        if let Err(e) = result.next().await {
            return Err(Self::internal_error("Failed to add episode", e));
        }

        let result = json!({
            "episode_id": episode_id,
            "linked_concepts": concepts,
            "valence": episode_valence,
        });

        Ok(CallToolResult {
            content: vec![Content::text(result.to_string())],
            structured_content: Some(result),
            is_error: Some(false),
            meta: None,
        })
    }

    #[tool(description = "Adds a relation between two concepts.")]
    pub async fn relation_add(
        &self,
        params: Parameters<RelationAddRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        let from = self
            .normalize_concept(&request.from)
            .ok_or_else(|| Self::invalid_params("Error: from: empty", json!({})))?;
        let to = self
            .normalize_concept(&request.to)
            .ok_or_else(|| Self::invalid_params("Error: to: empty", json!({})))?;

        if from == to {
            return Err(Self::invalid_params(
                "Error: relation: tautology",
                json!({"from": from, "to": to}),
            ));
        }

        let relation_label = self.map_relation_type(&request.relation_type)?;
        let relation_id = Uuid::new_v4().to_string();
        let now = Self::now_ms();

        let query_text = format!(
            "MERGE (a:Concept {{name: $from}})\n\
             ON CREATE SET a.valence = $valence, a.arousal_level = $arousal_level, a.accessed_at = $accessed_at\n\
             MERGE (b:Concept {{name: $to}})\n\
             ON CREATE SET b.valence = $valence, b.arousal_level = $arousal_level, b.accessed_at = $accessed_at\n\
             MERGE (a)-[r:{rel}]->(b)\n\
             SET r.id = coalesce(r.id, $id)\n\
             RETURN r.id AS id",
            rel = relation_label,
        );

        let query = query(query_text.as_str())
            .param("from", from.as_str())
            .param("to", to.as_str())
            .param("id", relation_id.as_str())
            .param("valence", DEFAULT_VALENCE)
            .param("arousal_level", INITIAL_AROUSAL_INDIRECT)
            .param("accessed_at", now);

        let mut result = self.graph.execute(query).await.map_err(|e| {
            Self::internal_error("Failed to add relation", e)
        })?;

        let id = if let Ok(Some(row)) = result.next().await {
            row.get("id").unwrap_or(relation_id.clone())
        } else {
            relation_id.clone()
        };

        let result = json!({
            "relation_id": id,
        });

        Ok(CallToolResult {
            content: vec![Content::text(result.to_string())],
            structured_content: Some(result),
            is_error: Some(false),
            meta: None,
        })
    }

    #[tool(description = "Recalls propositions from seed concepts up to max_hop.")]
    pub async fn recall_query(
        &self,
        params: Parameters<RecallQueryRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request = params.0;
        if request.max_hop == 0 {
            let result = json!({"propositions": []});
            return Ok(CallToolResult {
                content: vec![Content::text(result.to_string())],
                structured_content: Some(result),
                is_error: Some(false),
                meta: None,
            });
        }

        let mut seeds = Vec::new();
        for seed in &request.seeds {
            if let Some(normalized) = self.normalize_concept(seed) {
                seeds.push(normalized);
            }
        }

        if seeds.is_empty() {
            let result = json!({"propositions": []});
            return Ok(CallToolResult {
                content: vec![Content::text(result.to_string())],
                structured_content: Some(result),
                is_error: Some(false),
                meta: None,
            });
        }

        let now = Self::now_ms();
        let mut cache: HashMap<String, ConceptState> = HashMap::new();
        let mut visited: HashMap<String, u32> = HashMap::new();
        let mut queue: VecDeque<(String, u32)> = VecDeque::new();
        let mut propositions: HashMap<String, Proposition> = HashMap::new();

        for seed in seeds {
            if !visited.contains_key(&seed) {
                visited.insert(seed.clone(), 0);
                queue.push_back((seed, 0));
            }
        }

        while let Some((concept, hop)) = queue.pop_front() {
            if hop >= request.max_hop {
                continue;
            }

            let next_hop = hop + 1;

            let relations = self.fetch_relations(&concept).await?;
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

                if let Some(target_state) =
                    self.get_concept_state_cached(&mut cache, &target, now).await?
                {
                    let hop_decay = Self::hop_decay(next_hop, forward);
                    let arousal =
                        self.arousal(target_state.arousal_level, target_state.accessed_at, now);
                    let score = arousal * hop_decay;
                    let text = format!(
                        "{} {} {}",
                        edge.from,
                        self.render_relation_type(&edge.relation_type),
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
                        .get(&target)
                        .map(|existing| next_hop < *existing)
                        .unwrap_or(true)
                    {
                        visited.insert(target.clone(), next_hop);
                        queue.push_back((target.clone(), next_hop));
                    }

                    self.maybe_update_arousal(&mut cache, &target, hop_decay, now)
                        .await?;
                }
            }

            let episodes = self.fetch_episodes(&concept).await?;
            if !episodes.is_empty() {
                if let Some(concept_state) =
                    self.get_concept_state_cached(&mut cache, &concept, now).await?
                {
                    let hop_decay = Self::hop_decay(next_hop, true);
                    let arousal =
                        self.arousal(concept_state.arousal_level, concept_state.accessed_at, now);
                    let score = arousal * hop_decay;

                    for episode in episodes {
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

        let mut items: Vec<Proposition> = propositions.into_values().collect();
        items.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let result = json!({"propositions": items});

        Ok(CallToolResult {
            content: vec![Content::text(result.to_string())],
            structured_content: Some(result),
            is_error: Some(false),
            meta: None,
        })
    }
}

#[tool_handler]
impl ServerHandler for ConceptGraphService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Concept graph MCP server backed by Memgraph. Use concept strings as-is and query with seeds + max_hop.".into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            ..Default::default()
        }
    }
}
