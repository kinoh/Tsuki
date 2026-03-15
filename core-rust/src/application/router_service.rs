use futures::future::join_all;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    time::Instant,
};

use crate::activation_concept_graph::ActiveGraphNode;
use crate::app_state::AppState;
use crate::application::concept_activation_service::activate_concepts;
use crate::application::concept_retrieval_service::retrieve_concepts;
use crate::application::event_service::record_event;
use crate::application::module_bootstrap::Modules;
use crate::application::router_symbolization_service::symbolize;
use crate::event::contracts::{concept_graph_query, llm_error, router_state};
use crate::input_ingress::RouterInput;
use crate::llm::ToolError;
use crate::mcp::McpToolVisibility;
use crate::prompts::PromptOverrides;

const SATURATION_STEP: f64 = 0.24;
const SATURATION_MAX: f64 = 0.72;
const SATURATION_RECOVERY: f64 = 0.06;
const POST_HARD_DAMPEN_RATIO: f64 = 0.35;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HardTriggerResult {
    pub(crate) module: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RouterOutput {
    pub(crate) active_concepts_and_arousal: String,
    pub(crate) symbolized_text: String,
    pub(crate) module_scores: BTreeMap<String, f64>,
    pub(crate) saturation_penalties: BTreeMap<String, f64>,
    pub(crate) hard_effective_scores: BTreeMap<String, f64>,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
    pub(crate) mcp_visible_tools: Vec<String>,
    pub(crate) mcp_tool_visibility: Vec<McpToolVisibility>,
    pub(crate) hard_trigger_results: Vec<HardTriggerResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivationSnapshot {
    pub(crate) active_concepts_and_arousal: String,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
    /// Symbolized text of the input. For text-only input this equals the user text;
    /// for sensory input it includes the literal transcription of media.
    pub(crate) symbolized_text: String,
}

pub(crate) async fn run_router<F, Fut>(
    input: &RouterInput,
    module_instructions: &HashMap<String, String>,
    _modules: &Modules,
    state: &AppState,
    _overrides: &PromptOverrides,
    execute_submodule: F,
) -> RouterOutput
where
    F: Fn(&str, &ActivationSnapshot, &str, Option<&str>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolError>> + Send,
{
    let router_started = Instant::now();
    let active_module_names = module_instructions.keys().cloned().collect::<Vec<_>>();
    let concept_limit = state.config.router.query_terms_max.max(1);
    let active_state_limit = state.config.router.active_state_limit.max(1);

    let symbolization = symbolize(input, state.services.router_symbolizer.as_ref()).await;
    if let Some(err) = &symbolization.error {
        emit_router_debug_error(state, "symbolize", err).await;
    }

    let retrieval = retrieve_concepts(
        &symbolization.text,
        input,
        concept_limit,
        &state.config.router.multimodal_embedding,
        state.services.activation_concept_graph.as_ref(),
    )
    .await;
    for err in &retrieval.errors {
        emit_router_debug_error(state, "concept_retrieval", err).await;
    }

    let activation = activate_concepts(
        &retrieval.candidate_concepts,
        active_state_limit,
        state.services.activation_concept_graph.as_ref(),
    )
    .await;
    for err in &activation.errors {
        emit_router_debug_error(state, "concept_activation", err).await;
    }

    emit_concept_graph_query_event(
        state,
        &symbolization.text,
        active_state_limit,
        &retrieval.text_candidate_concepts,
        &retrieval.multimodal_candidate_concepts,
        &retrieval.candidate_concepts,
        retrieval.candidate_source.as_str(),
        &retrieval.candidate_concepts,
        &activation.active_concepts_and_arousal,
        &symbolization.text,
    )
    .await;

    let activation_sources = collect_submodule_activation_sources(&retrieval.candidate_concepts);
    if !activation_sources.is_empty() {
        if let Err(err) = state
            .services
            .activation_concept_graph
            .activate_related_submodules(activation_sources)
            .await
        {
            emit_router_debug_error(state, "activate_related_submodules", &err).await;
        }
    }

    let scores = compute_module_scores_from_concept_activation(&active_module_names, state).await;
    let module_scores = scores
        .iter()
        .map(|(name, score)| (name.clone(), *score))
        .collect::<BTreeMap<_, _>>();
    let saturation_levels = read_saturation_levels(state, &active_module_names).await;
    let (hard_scores, saturation_penalties, hard_effective_scores) =
        apply_saturation_penalty(&scores, &saturation_levels);
    let hard_triggers =
        select_modules_by_threshold(&hard_scores, state.config.router.hard_trigger_threshold);
    let soft_recommendations =
        select_modules_by_threshold(&scores, state.config.router.recommendation_threshold);
    let mcp_tool_visibility = state
        .services
        .mcp_registry
        .resolve_visibility(
            state.services.activation_concept_graph.as_ref(),
            state.config.router.recommendation_threshold,
        )
        .await;
    let mcp_visible_tools = mcp_tool_visibility
        .iter()
        .filter(|item| item.visible)
        .map(|item| item.runtime_tool_name.clone())
        .collect::<Vec<_>>();
    let activation_snapshot = ActivationSnapshot {
        active_concepts_and_arousal: activation.active_concepts_and_arousal.clone(),
        hard_triggers: hard_triggers.clone(),
        soft_recommendations: soft_recommendations.clone(),
        symbolized_text: symbolization.text.clone(),
    };
    let hard_trigger_results = run_hard_triggers(
        &activation_snapshot,
        module_instructions,
        &execute_submodule,
    )
    .await;
    update_saturation_levels(state, &active_module_names, &hard_triggers).await;
    dampen_hard_triggered_submodule_arousal(state, &hard_triggers).await;

    let router_output = RouterOutput {
        active_concepts_and_arousal: activation.active_concepts_and_arousal,
        symbolized_text: symbolization.text,
        module_scores,
        saturation_penalties,
        hard_effective_scores,
        hard_triggers,
        soft_recommendations,
        mcp_visible_tools,
        mcp_tool_visibility,
        hard_trigger_results,
    };
    let router_event = router_state(
        serde_json::to_value(&router_output)
            .unwrap_or_else(|_| json!({ "error": "router_output_serialize_failed" })),
    );
    record_event(state, router_event).await;
    println!(
        "PERF router stage=end total_ms={} active_modules={} hard_triggers={} hard_results={} soft_recommendations={} concepts_len={}",
        router_started.elapsed().as_millis(),
        active_module_names.len(),
        router_output.hard_triggers.len(),
        router_output.hard_trigger_results.len(),
        router_output.soft_recommendations.len(),
        router_output.active_concepts_and_arousal.len()
    );
    router_output
}

pub(crate) fn activation_snapshot_from_router_output(
    router_output: &RouterOutput,
) -> ActivationSnapshot {
    ActivationSnapshot {
        active_concepts_and_arousal: router_output.active_concepts_and_arousal.clone(),
        hard_triggers: router_output.hard_triggers.clone(),
        soft_recommendations: router_output.soft_recommendations.clone(),
        symbolized_text: router_output.symbolized_text.clone(),
    }
}

async fn run_hard_triggers<F, Fut>(
    activation_snapshot: &ActivationSnapshot,
    module_instructions: &HashMap<String, String>,
    execute_submodule: &F,
) -> Vec<HardTriggerResult>
where
    F: Fn(&str, &ActivationSnapshot, &str, Option<&str>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolError>> + Send,
{
    if activation_snapshot.hard_triggers.is_empty() {
        return Vec::new();
    }
    let started = Instant::now();
    let runs = activation_snapshot
        .hard_triggers
        .iter()
        .filter_map(|module_name| {
            module_instructions
                .get(module_name)
                .map(|instructions| (module_name.clone(), instructions.clone()))
        })
        .map(|(module_name, instructions)| async move {
            let module_started = Instant::now();
            let result = execute_submodule(
                &module_name,
                activation_snapshot,
                &instructions,
                Some("hard_trigger"),
            )
            .await;
            println!(
                "PERF router.hard_trigger module={} ms={} ok={}",
                module_name,
                module_started.elapsed().as_millis(),
                result.is_ok()
            );
            (module_name, result)
        })
        .collect::<Vec<_>>();
    let outputs = join_all(runs)
        .await
        .into_iter()
        .map(|(module, result)| match result {
            Ok(text) => HardTriggerResult { module, text },
            Err(err) => HardTriggerResult {
                module,
                text: format!("error: {}", err),
            },
        })
        .collect::<Vec<_>>();
    println!(
        "PERF router.hard_trigger stage=end total_ms={} count={}",
        started.elapsed().as_millis(),
        outputs.len()
    );
    outputs
}

async fn compute_module_scores_from_concept_activation(
    active_module_names: &[String],
    state: &AppState,
) -> Vec<(String, f64)> {
    let submodule_concepts = active_module_names
        .iter()
        .map(|name| format!("submodule:{}", name))
        .collect::<Vec<_>>();
    let concept_scores = state
        .services
        .activation_concept_graph
        .concept_activation(&submodule_concepts)
        .await
        .unwrap_or_default();
    map_module_scores_from_concept_scores(active_module_names, &concept_scores)
}

fn map_module_scores_from_concept_scores(
    active_module_names: &[String],
    concept_scores: &HashMap<String, f64>,
) -> Vec<(String, f64)> {
    let mut scored = active_module_names
        .iter()
        .map(|name| {
            let concept_name = format!("submodule:{}", name);
            let score = concept_scores
                .get(concept_name.as_str())
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            (name.clone(), score)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored
}

fn select_modules_by_threshold(scores: &[(String, f64)], threshold: f32) -> Vec<String> {
    let threshold = (threshold as f64).clamp(0.0, 1.0);
    scores
        .iter()
        .filter(|(_, score)| *score >= threshold)
        .map(|(name, _)| name.clone())
        .collect()
}

async fn read_saturation_levels(
    state: &AppState,
    active_module_names: &[String],
) -> HashMap<String, f64> {
    let guard = state.runtime.submodule_saturation_levels.read().await;
    active_module_names
        .iter()
        .filter_map(|name| {
            guard
                .get(name)
                .copied()
                .map(|value| (name.clone(), value.clamp(0.0, SATURATION_MAX)))
        })
        .collect::<HashMap<_, _>>()
}

fn apply_saturation_penalty(
    scores: &[(String, f64)],
    saturation_levels: &HashMap<String, f64>,
) -> (
    Vec<(String, f64)>,
    BTreeMap<String, f64>,
    BTreeMap<String, f64>,
) {
    let mut inhibited = Vec::<(String, f64)>::with_capacity(scores.len());
    let mut penalties = BTreeMap::<String, f64>::new();
    let mut effective = BTreeMap::<String, f64>::new();
    for (name, score) in scores {
        let penalty = saturation_levels
            .get(name.as_str())
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, SATURATION_MAX);
        let adjusted = (score - penalty).clamp(0.0, 1.0);
        inhibited.push((name.clone(), adjusted));
        penalties.insert(name.clone(), penalty);
        effective.insert(name.clone(), adjusted);
    }
    inhibited.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    (inhibited, penalties, effective)
}

async fn update_saturation_levels(
    state: &AppState,
    active_module_names: &[String],
    hard_triggers: &[String],
) {
    let hard_set = hard_triggers.iter().cloned().collect::<HashSet<_>>();
    let mut guard = state.runtime.submodule_saturation_levels.write().await;
    for name in active_module_names {
        let current = guard
            .get(name.as_str())
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, SATURATION_MAX);
        let next = if hard_set.contains(name.as_str()) {
            (current + SATURATION_STEP).min(SATURATION_MAX)
        } else {
            (current - SATURATION_RECOVERY).max(0.0)
        };
        if next <= 0.0 {
            guard.remove(name.as_str());
        } else {
            guard.insert(name.clone(), next);
        }
    }
}

async fn dampen_hard_triggered_submodule_arousal(state: &AppState, hard_triggers: &[String]) {
    for module_name in hard_triggers {
        let concept = format!("submodule:{}", module_name);
        if let Err(err) = state
            .services
            .activation_concept_graph
            .dampen_concept_arousal(concept, POST_HARD_DAMPEN_RATIO)
            .await
        {
            emit_router_debug_error(state, "dampen_concept_arousal", &err).await;
        }
    }
}

fn collect_submodule_activation_sources(selected_seeds: &[String]) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for seed in selected_seeds {
        let trimmed = seed.trim();
        if trimmed.is_empty() || trimmed.starts_with("submodule:") {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

async fn emit_router_debug_error(state: &AppState, context: &str, error: &str) {
    let event = llm_error(
        "router",
        json!({
            "mode": "runtime",
            "context": context,
            "error": error,
        }),
        vec!["mode:runtime".to_string()],
    );
    record_event(state, event).await;
}

async fn emit_concept_graph_query_event(
    state: &AppState,
    query_text: &str,
    active_state_limit: usize,
    text_candidate_concepts: &[String],
    multimodal_candidate_concepts: &[String],
    candidate_concepts: &[String],
    candidate_source: &str,
    selected_seeds: &[String],
    active_concepts_and_arousal: &str,
    symbolized_text: &str,
) {
    let event = concept_graph_query(json!({
        "query_text": query_text,
        "symbolized_text": symbolized_text,
        "active_state_limit": active_state_limit,
        "text_result_concepts": text_candidate_concepts,
        "multimodal_result_concepts": multimodal_candidate_concepts,
        "result_concepts": candidate_concepts,
        "candidate_source": candidate_source,
        "selected_seeds": selected_seeds,
        "active_concepts_and_arousal": active_concepts_and_arousal,
    }));
    record_event(state, event).await;
}

#[allow(dead_code)]
fn parse_recall_seeds(text: &str, max: usize) -> Vec<String> {
    let mut seeds = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for raw in text.lines() {
        let mut line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("none") || line.eq_ignore_ascii_case("(empty response)") {
            continue;
        }
        line = line
            .trim_start_matches("-")
            .trim_start_matches("*")
            .trim_start_matches("•")
            .trim();
        line = line
            .strip_prefix("seed=")
            .or_else(|| line.strip_prefix("seed:"))
            .unwrap_or(line)
            .trim();
        if let Some((prefix, _)) = line.split_once("\tscore=") {
            line = prefix.trim();
        }
        line = line.trim_matches('"').trim_matches('\'').trim();
        if line.is_empty() {
            continue;
        }
        let key = line.to_lowercase();
        if seen.insert(key) {
            seeds.push(line.to_string());
        }
        if seeds.len() >= max {
            break;
        }
    }
    seeds
}

#[allow(dead_code)]
fn render_active_nodes_as_text(nodes: &[ActiveGraphNode]) -> String {
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
    use super::*;

    #[test]
    fn select_modules_by_threshold_filters_by_score() {
        let scores = vec![
            ("curiosity".to_string(), 0.92),
            ("self_preservation".to_string(), 0.72),
            ("social_approval".to_string(), 0.58),
        ];
        let hard = select_modules_by_threshold(&scores, 0.85);
        let soft = select_modules_by_threshold(&scores, 0.60);
        assert_eq!(hard, vec!["curiosity".to_string()]);
        assert_eq!(
            soft,
            vec!["curiosity".to_string(), "self_preservation".to_string()]
        );
    }

    #[test]
    fn parse_recall_seeds_ignores_noise() {
        let text = "- seed=京都\n* 大阪\tscore=0.42\nnone\n";
        let seeds = parse_recall_seeds(text, 4);
        assert_eq!(seeds, vec!["京都".to_string(), "大阪".to_string()]);
    }

    #[test]
    fn render_active_nodes_as_text_formats_lines() {
        let nodes = vec![ActiveGraphNode {
            label: "shell command".to_string(),
            arousal: 0.973,
        }];
        let text = render_active_nodes_as_text(&nodes);
        assert_eq!(text, "shell command\tarousal=0.97");
    }

    #[test]
    fn map_module_scores_reads_submodule_concept_activation_only() {
        let modules = vec![
            "curiosity".to_string(),
            "self_preservation".to_string(),
            "social_approval".to_string(),
        ];
        let concept_scores = HashMap::from([
            ("submodule:curiosity".to_string(), 0.91),
            ("submodule:self_preservation".to_string(), 0.41),
            ("関係ない概念".to_string(), 1.0),
        ]);
        let scores = map_module_scores_from_concept_scores(&modules, &concept_scores);
        assert_eq!(
            scores,
            vec![
                ("curiosity".to_string(), 0.91),
                ("self_preservation".to_string(), 0.41),
                ("social_approval".to_string(), 0.0),
            ]
        );
    }
}
