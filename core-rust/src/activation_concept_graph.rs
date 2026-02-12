use neo4rs::{query, Graph};
use std::collections::HashSet;
use std::sync::Arc;

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
        Ok(Self::new(Arc::new(graph), arousal_tau_ms))
    }

    pub(crate) async fn concept_search(
        &self,
        keywords: &[String],
        limit: usize,
    ) -> Result<Vec<String>, String> {
        let limit = limit.max(1).min(200);
        let normalized_keywords = keywords
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
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
            .param("keywords", normalized_keywords.clone())
            .param("limit", limit as i64);
            let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
            while let Ok(Some(row)) = result.next().await {
                let name: String = row.get("name").unwrap_or_default();
                if name.is_empty() || !seen.insert(name.clone()) {
                    continue;
                }
                concepts.push(name);
                if concepts.len() >= limit {
                    return Ok(concepts);
                }
            }
        }

        let remaining = limit.saturating_sub(concepts.len());
        if remaining == 0 {
            return Ok(concepts);
        }

        let q = query(
            "MATCH (c:Concept)
             RETURN c.name AS name, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at",
        );
        let mut result = self.graph.execute(q).await.map_err(|err| err.to_string())?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0)
            .max(1);
        let mut scored = Vec::<(String, f64)>::new();
        while let Ok(Some(row)) = result.next().await {
            let name: String = row.get("name").unwrap_or_default();
            if name.is_empty() || seen.contains(&name) {
                continue;
            }
            let arousal_level: f64 = row.get("arousal_level").unwrap_or(0.0);
            let accessed_at: i64 = row.get("accessed_at").unwrap_or(0);
            let delta_ms = (now_ms - accessed_at.max(0)).max(0) as f64;
            let arousal = arousal_level * (-delta_ms / self.arousal_tau_ms).exp();
            scored.push((name, arousal));
        }
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        for (name, _) in scored.into_iter().take(remaining) {
            if seen.insert(name.clone()) {
                concepts.push(name);
            }
        }
        Ok(concepts)
    }
}
