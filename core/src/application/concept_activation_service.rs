use crate::activation_concept_graph::ConceptGraphOps;
use crate::activation_concept_graph::{ActiveGraphNode, ConceptGraphActivationReader};

const RECALL_MAX_HOP: u32 = 2;

pub(crate) struct ConceptActivationResult {
    pub(crate) active_concepts_and_arousal: String,
    pub(crate) errors: Vec<String>,
}

pub(crate) async fn activate_concepts<G>(
    seeds: &[String],
    active_state_limit: usize,
    graph: &G,
    dry_run: bool,
) -> ConceptActivationResult
where
    G: ConceptGraphActivationReader + ConceptGraphOps + ?Sized,
{
    let mut errors = Vec::<String>::new();

    if !seeds.is_empty() {
        if let Err(err) = graph
            .recall_query(seeds.to_vec(), RECALL_MAX_HOP, dry_run)
            .await
        {
            errors.push(format!("recall_query seeds={:?}: {}", seeds, err));
        }
    }

    let active_concepts_and_arousal = match graph.active_nodes(active_state_limit).await {
        Ok(nodes) => render_active_nodes(nodes.as_slice()),
        Err(err) => {
            errors.push(format!("active_nodes: {}", err));
            "none".to_string()
        }
    };

    ConceptActivationResult {
        active_concepts_and_arousal,
        errors,
    }
}

fn render_active_nodes(nodes: &[ActiveGraphNode]) -> String {
    let lines = nodes
        .iter()
        .filter_map(|node| {
            let label = node.label.trim();
            if label.is_empty() {
                return None;
            }
            Some(format!(
                "{}\tarousal={:.2}",
                label,
                node.arousal.clamp(0.0, 1.0)
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use serde_json::Value;

    use super::*;
    use crate::activation_concept_graph::VisibleSkill;
    use crate::input_ingress::RouterInput;

    struct MockGraph {
        active_nodes: Result<Vec<ActiveGraphNode>, String>,
        recall_error: Option<String>,
    }

    #[async_trait]
    impl ConceptGraphActivationReader for MockGraph {
        async fn concept_search(
            &self,
            _input_text: &str,
            _limit: usize,
        ) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }

        async fn concept_search_multimodal(
            &self,
            _input: &RouterInput,
            _limit: usize,
        ) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }

        async fn active_nodes(&self, _limit: usize) -> Result<Vec<ActiveGraphNode>, String> {
            self.active_nodes.clone()
        }

        async fn concept_activation(
            &self,
            _concepts: &[String],
        ) -> Result<HashMap<String, f64>, String> {
            Ok(HashMap::new())
        }

        async fn visible_skills(
            &self,
            _threshold: f64,
            _limit: usize,
        ) -> Result<Vec<VisibleSkill>, String> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl ConceptGraphOps for MockGraph {
        async fn concept_upsert(&self, _concept: String) -> Result<Value, String> {
            Ok(Value::Null)
        }
        async fn skill_index_upsert(
            &self,
            _skill_name: String,
            _summary: String,
            _body_state_key: String,
            _required_mcp_tools: Vec<String>,
            _enabled: bool,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        async fn skill_index_replace_triggers(
            &self,
            _skill_name: String,
            _trigger_concepts: Vec<String>,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        async fn update_affect(
            &self,
            _target: String,
            _valence_delta: f64,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        async fn activate_related_submodules(
            &self,
            _concepts: Vec<String>,
        ) -> Result<HashMap<String, f64>, String> {
            Ok(HashMap::new())
        }
        async fn activate_related_skills(
            &self,
            _concepts: Vec<String>,
        ) -> Result<HashMap<String, f64>, String> {
            Ok(HashMap::new())
        }
        async fn dampen_concept_arousal(
            &self,
            _concept: String,
            _ratio: f64,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        async fn episode_add(
            &self,
            _summary: String,
            _concepts: Vec<String>,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        async fn relation_add(
            &self,
            _from: String,
            _to: String,
            _relation_type: String,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        async fn recall_query(
            &self,
            _seeds: Vec<String>,
            _max_hop: u32,
            _dry_run: bool,
        ) -> Result<Value, String> {
            match &self.recall_error {
                Some(err) => Err(err.clone()),
                None => Ok(Value::Null),
            }
        }
    }

    #[tokio::test]
    async fn renders_active_nodes_as_text() {
        let graph = MockGraph {
            active_nodes: Ok(vec![
                ActiveGraphNode {
                    label: "好奇心".to_string(),
                    arousal: 0.85,
                },
                ActiveGraphNode {
                    label: "音楽".to_string(),
                    arousal: 0.5,
                },
            ]),
            recall_error: None,
        };
        let result = activate_concepts(&["好奇心".to_string()], 8, &graph, false).await;
        assert_eq!(
            result.active_concepts_and_arousal,
            "好奇心\tarousal=0.85\n音楽\tarousal=0.50"
        );
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn returns_none_when_no_active_nodes() {
        let graph = MockGraph {
            active_nodes: Ok(Vec::new()),
            recall_error: None,
        };
        let result = activate_concepts(&[], 8, &graph, false).await;
        assert_eq!(result.active_concepts_and_arousal, "none");
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn skips_recall_when_seeds_empty() {
        let graph = MockGraph {
            active_nodes: Ok(Vec::new()),
            recall_error: Some("should not be called".to_string()),
        };
        let result = activate_concepts(&[], 8, &graph, false).await;
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn records_recall_error_but_still_returns_active_nodes() {
        let graph = MockGraph {
            active_nodes: Ok(vec![ActiveGraphNode {
                label: "concept".to_string(),
                arousal: 0.6,
            }]),
            recall_error: Some("graph unreachable".to_string()),
        };
        let result = activate_concepts(&["seed".to_string()], 8, &graph, false).await;
        assert_eq!(result.active_concepts_and_arousal, "concept\tarousal=0.60");
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("graph unreachable"));
    }

    #[tokio::test]
    async fn records_active_nodes_error() {
        let graph = MockGraph {
            active_nodes: Err("db down".to_string()),
            recall_error: None,
        };
        let result = activate_concepts(&[], 8, &graph, false).await;
        assert_eq!(result.active_concepts_and_arousal, "none");
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("db down"));
    }

    #[test]
    fn render_active_nodes_formats_arousal() {
        let nodes = vec![ActiveGraphNode {
            label: "shell command".to_string(),
            arousal: 0.973,
        }];
        assert_eq!(render_active_nodes(&nodes), "shell command\tarousal=0.97");
    }

    #[test]
    fn render_active_nodes_skips_empty_labels() {
        let nodes = vec![
            ActiveGraphNode {
                label: "  ".to_string(),
                arousal: 0.9,
            },
            ActiveGraphNode {
                label: "valid".to_string(),
                arousal: 0.5,
            },
        ];
        assert_eq!(render_active_nodes(&nodes), "valid\tarousal=0.50");
    }
}
