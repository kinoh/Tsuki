use async_trait::async_trait;
use neo4rs::{query, Graph};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use time::{OffsetDateTime, UtcOffset};

const DEFAULT_VALENCE: f64 = 0.0;
const DEFAULT_AROUSAL_LEVEL: f64 = 0.0;
const INITIAL_AROUSAL_UPSERT: f64 = 0.5;
const INITIAL_AROUSAL_INDIRECT: f64 = 0.25;
const DEFAULT_ACCESSED_AT: i64 = 0;
const DEFAULT_RELATION_WEIGHT: f64 = 0.25;
const RELATION_WEIGHT_ALPHA: f64 = 0.2;
const REVERSE_PENALTY: f64 = 0.5;

#[async_trait]
pub(crate) trait ConceptGraphActivationReader: Send + Sync {
    async fn concept_search(
        &self,
        keywords: &[String],
        limit: usize,
    ) -> Result<Vec<String>, String>;
    async fn concept_activation(&self, concepts: &[String])
        -> Result<HashMap<String, f64>, String>;
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
    async fn update_affect(&self, target: String, valence_delta: f64) -> Result<Value, String>;
    async fn activate_related_submodules(
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
    async fn recall_query(&self, seeds: Vec<String>, max_hop: u32) -> Result<Value, String>;
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

pub(crate) struct ActivationConceptGraphStore {
    graph: Arc<Graph>,
    arousal_tau_ms: f64,
}

impl ActivationConceptGraphStore {
    pub(crate) fn new(graph: Arc<Graph>, arousal_tau_ms: f64) -> Self {
        Self {
            graph,
            arousal_tau_ms: arousal_tau_ms.max(1.0),
        }
    }

    pub(crate) async fn connect(
        uri: String,
        user: String,
        password: String,
        arousal_tau_ms: f64,
    ) -> Result<Self, String> {
        let graph = Graph::new(uri, user, password).map_err(|err| err.to_string())?;
        Self::ensure_constraints(&graph).await?;
        Ok(Self::new(Arc::new(graph), arousal_tau_ms))
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
}

#[async_trait]
impl ConceptGraphActivationReader for ActivationConceptGraphStore {
    async fn concept_search(
        &self,
        keywords: &[String],
        limit: usize,
    ) -> Result<Vec<String>, String> {
        let limit = Self::clamp_limit(limit, 50, 200);
        let normalized_keywords = keywords
            .iter()
            .filter_map(|value| Self::normalize_non_empty(value))
            .collect::<Vec<_>>();
        let mut concepts = Vec::new();
        let mut seen = HashSet::new();
        if !normalized_keywords.is_empty() {
            let q = query(
                "UNWIND $keywords AS kw
                 MATCH (c:Concept)
                 WHERE toLower(c.name) CONTAINS toLower(kw)
                 RETURN DISTINCT c.name AS name
                 ORDER BY name
                 LIMIT $limit",
            )
            .param("keywords", normalized_keywords)
            .param("limit", limit as i64);
            let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
            while let Ok(Some(row)) = result.next().await {
                let name: String = row.get("name").unwrap_or_default();
                if name.is_empty() || !seen.insert(name.clone()) {
                    continue;
                }
                concepts.push(name);
                if concepts.len() >= limit {
                    break;
                }
            }
        }
        if concepts.len() < limit {
            let now = self.now_ms();
            let remaining = limit - concepts.len();
            let fallback = self.fetch_arousal_ranked(&seen, remaining, now).await?;
            for name in fallback {
                if concepts.len() >= limit {
                    break;
                }
                if seen.insert(name.clone()) {
                    concepts.push(name);
                }
            }
        }
        Ok(concepts)
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
        Ok(json!({
            "concept_id": concept,
            "created": created,
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
                if !target.starts_with("submodule:") {
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

    async fn recall_query(&self, seeds: Vec<String>, max_hop: u32) -> Result<Value, String> {
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
                    if !target.starts_with("submodule:") {
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
             RETURN c.name AS name, c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at",
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
            let valence: f64 = row.get("valence").unwrap_or(DEFAULT_VALENCE);
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(DEFAULT_AROUSAL_LEVEL);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(DEFAULT_ACCESSED_AT);
            let arousal = Self::round_score(self.arousal(arousal_level, accessed_at, now));
            items.push(json!({
                "name": name,
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
        Ok(Some(json!({
            "name": concept,
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
