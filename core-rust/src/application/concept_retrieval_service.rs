use std::collections::HashSet;

use crate::activation_concept_graph::ConceptGraphActivationReader;
use crate::config::RouterMultimodalEmbeddingConfig;
use crate::input_ingress::RouterInput;

pub(crate) struct ConceptRetrievalResult {
    pub(crate) candidate_concepts: Vec<String>,
    pub(crate) text_candidate_concepts: Vec<String>,
    pub(crate) multimodal_candidate_concepts: Vec<String>,
    pub(crate) candidate_source: String,
    pub(crate) errors: Vec<String>,
}

/// Retrieves concept candidates from the graph.
///
/// `query_text` is the symbolized text used for text embedding search. This may differ from
/// `input.display_text()` when symbolization has converted multimodal input into a literal
/// description. `input` is the original router input used for multimodal embedding.
pub(crate) async fn retrieve_concepts(
    query_text: &str,
    input: &RouterInput,
    limit: usize,
    multimodal_config: &RouterMultimodalEmbeddingConfig,
    graph: &dyn ConceptGraphActivationReader,
) -> ConceptRetrievalResult {
    let query_text = query_text.trim();
    if query_text.is_empty() {
        return ConceptRetrievalResult {
            candidate_concepts: Vec::new(),
            text_candidate_concepts: Vec::new(),
            multimodal_candidate_concepts: Vec::new(),
            candidate_source: "none".to_string(),
            errors: Vec::new(),
        };
    }

    let text_candidate_concepts = graph
        .concept_search(query_text, limit)
        .await
        .unwrap_or_default();

    let requested_multimodal = multimodal_config.enabled
        && (multimodal_config.shadow_enabled
            || !multimodal_config
                .primary_source
                .trim()
                .eq_ignore_ascii_case("text"));

    let mut errors = Vec::<String>::new();
    let multimodal_candidate_concepts = if requested_multimodal {
        match graph.concept_search_multimodal(input, limit).await {
            Ok(items) => items,
            Err(err) => {
                errors.push(err);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let candidate_source = resolve_candidate_source(multimodal_config.primary_source.as_str());
    let candidate_concepts = match candidate_source.as_str() {
        "multimodal" => multimodal_candidate_concepts.clone(),
        "hybrid" => merge_unique(&multimodal_candidate_concepts, &text_candidate_concepts),
        _ => text_candidate_concepts.clone(),
    };

    let record_multimodal = multimodal_config.shadow_enabled || candidate_source != "text";

    ConceptRetrievalResult {
        candidate_concepts,
        text_candidate_concepts,
        multimodal_candidate_concepts: if record_multimodal {
            multimodal_candidate_concepts
        } else {
            Vec::new()
        },
        candidate_source,
        errors,
    }
}

fn resolve_candidate_source(configured: &str) -> String {
    match configured.trim().to_ascii_lowercase().as_str() {
        "multimodal" => "multimodal".to_string(),
        "hybrid" => "hybrid".to_string(),
        _ => "text".to_string(),
    }
}

fn merge_unique(primary: &[String], secondary: &[String]) -> Vec<String> {
    let mut merged = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for value in primary.iter().chain(secondary.iter()) {
        if seen.insert(value.clone()) {
            merged.push(value.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::activation_concept_graph::ActiveGraphNode;

    struct MockGraph {
        text_results: Vec<String>,
        multimodal_results: Result<Vec<String>, String>,
    }

    #[async_trait::async_trait]
    impl ConceptGraphActivationReader for MockGraph {
        async fn concept_search(
            &self,
            _input_text: &str,
            _limit: usize,
        ) -> Result<Vec<String>, String> {
            Ok(self.text_results.clone())
        }

        async fn concept_search_multimodal(
            &self,
            _input: &RouterInput,
            _limit: usize,
        ) -> Result<Vec<String>, String> {
            self.multimodal_results.clone()
        }

        async fn active_nodes(&self, _limit: usize) -> Result<Vec<ActiveGraphNode>, String> {
            Ok(Vec::new())
        }

        async fn concept_activation(
            &self,
            _concepts: &[String],
        ) -> Result<HashMap<String, f64>, String> {
            Ok(HashMap::new())
        }
    }

    fn text_input(text: &str) -> RouterInput {
        RouterInput::from_text("user", text)
    }

    #[tokio::test]
    async fn returns_empty_when_query_text_is_empty() {
        let graph = MockGraph {
            text_results: vec!["concept_a".to_string()],
            multimodal_results: Ok(vec!["concept_b".to_string()]),
        };
        let config = RouterMultimodalEmbeddingConfig {
            enabled: true,
            ..Default::default()
        };
        let result = retrieve_concepts("", &text_input(""), 8, &config, &graph).await;
        assert_eq!(result.candidate_source, "none");
        assert!(result.candidate_concepts.is_empty());
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn text_only_when_multimodal_disabled() {
        let graph = MockGraph {
            text_results: vec!["concept_a".to_string()],
            multimodal_results: Ok(vec!["concept_b".to_string()]),
        };
        let config = RouterMultimodalEmbeddingConfig::default(); // enabled=false
        let result = retrieve_concepts("hello", &text_input("hello"), 8, &config, &graph).await;
        assert_eq!(result.candidate_source, "text");
        assert_eq!(result.candidate_concepts, vec!["concept_a"]);
        assert!(result.multimodal_candidate_concepts.is_empty());
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn multimodal_primary_returns_multimodal_candidates() {
        let graph = MockGraph {
            text_results: vec!["concept_a".to_string()],
            multimodal_results: Ok(vec!["concept_b".to_string()]),
        };
        let config = RouterMultimodalEmbeddingConfig {
            enabled: true,
            primary_source: "multimodal".to_string(),
            ..Default::default()
        };
        let result = retrieve_concepts("hello", &text_input("hello"), 8, &config, &graph).await;
        assert_eq!(result.candidate_source, "multimodal");
        assert_eq!(result.candidate_concepts, vec!["concept_b"]);
        assert_eq!(result.multimodal_candidate_concepts, vec!["concept_b"]);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn hybrid_merges_and_deduplicates() {
        let graph = MockGraph {
            text_results: vec!["concept_a".to_string(), "shared".to_string()],
            multimodal_results: Ok(vec!["shared".to_string(), "concept_b".to_string()]),
        };
        let config = RouterMultimodalEmbeddingConfig {
            enabled: true,
            primary_source: "hybrid".to_string(),
            ..Default::default()
        };
        let result = retrieve_concepts("hello", &text_input("hello"), 8, &config, &graph).await;
        assert_eq!(result.candidate_source, "hybrid");
        // multimodal-first, then text, deduped
        assert_eq!(
            result.candidate_concepts,
            vec!["shared", "concept_b", "concept_a"]
        );
    }

    #[tokio::test]
    async fn shadow_mode_records_both_but_uses_text_as_primary() {
        let graph = MockGraph {
            text_results: vec!["concept_a".to_string()],
            multimodal_results: Ok(vec!["concept_b".to_string()]),
        };
        let config = RouterMultimodalEmbeddingConfig {
            enabled: true,
            shadow_enabled: true,
            primary_source: "text".to_string(),
            ..Default::default()
        };
        let result = retrieve_concepts("hello", &text_input("hello"), 8, &config, &graph).await;
        assert_eq!(result.candidate_source, "text");
        assert_eq!(result.candidate_concepts, vec!["concept_a"]);
        assert_eq!(result.multimodal_candidate_concepts, vec!["concept_b"]);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn multimodal_error_is_recorded_and_falls_back_to_empty() {
        let graph = MockGraph {
            text_results: vec!["concept_a".to_string()],
            multimodal_results: Err("embedding failed".to_string()),
        };
        let config = RouterMultimodalEmbeddingConfig {
            enabled: true,
            primary_source: "multimodal".to_string(),
            ..Default::default()
        };
        let result = retrieve_concepts("hello", &text_input("hello"), 8, &config, &graph).await;
        assert!(result.multimodal_candidate_concepts.is_empty());
        assert_eq!(result.errors, vec!["embedding failed"]);
    }
}
