use futures::future::join_all;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
};

use crate::event::build_event;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig, ToolError};
use crate::{record_event, AppState, ModuleRuntime, Modules};

const ROUTER_QUERY_TERMS_MAX: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HardTriggerResult {
    pub(crate) module: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RouterOutput {
    pub(crate) activation_query_terms: Vec<String>,
    pub(crate) concepts: Vec<String>,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
    pub(crate) hard_trigger_results: Vec<HardTriggerResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivationSnapshot {
    pub(crate) concepts: Vec<String>,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
}

pub(crate) async fn run_router<F, Fut>(
    input_text: &str,
    module_instructions: &HashMap<String, String>,
    modules: &Modules,
    state: &AppState,
    execute_submodule: F,
) -> RouterOutput
where
    F: Fn(&str, &ActivationSnapshot, &str, Option<&str>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolError>> + Send,
{
    let active_module_names = module_instructions.keys().cloned().collect::<Vec<_>>();
    let activation_query_terms =
        infer_activation_query_terms(input_text, &active_module_names, modules, state).await;
    let concept_limit = state.router.concept_top_n.max(1);
    let concepts = match state
        .activation_concept_graph
        .concept_search(&activation_query_terms, concept_limit)
        .await
    {
        Ok(values) => {
            emit_concept_graph_query_event(
                state,
                &activation_query_terms,
                concept_limit,
                &values,
                None,
            )
            .await;
            values
        }
        Err(err) => {
            println!(
                "ACTIVATION_CONCEPT_GRAPH_ERROR op=concept_search error={}",
                err
            );
            let error = err.to_string();
            emit_concept_graph_query_event(
                state,
                &activation_query_terms,
                concept_limit,
                &[],
                Some(&error),
            )
            .await;
            Vec::new()
        }
    };

    let scores = compute_module_scores_minimal(
        input_text,
        &activation_query_terms,
        &concepts,
        &active_module_names,
    );
    let hard_triggers = select_modules_by_threshold(&scores, state.router.hard_trigger_threshold);
    let soft_recommendations =
        select_modules_by_threshold(&scores, state.router.recommendation_threshold);
    let activation_snapshot = ActivationSnapshot {
        concepts: concepts.clone(),
        hard_triggers: hard_triggers.clone(),
        soft_recommendations: soft_recommendations.clone(),
    };
    let hard_trigger_results = run_hard_triggers(
        &activation_snapshot,
        module_instructions,
        &execute_submodule,
    )
    .await;

    let router_output = RouterOutput {
        activation_query_terms,
        concepts,
        hard_triggers,
        soft_recommendations,
        hard_trigger_results,
    };
    let router_event = build_event(
        "router",
        "state",
        serde_json::to_value(&router_output)
            .unwrap_or_else(|_| json!({ "error": "router_output_serialize_failed" })),
        vec!["router".to_string()],
    );
    record_event(state, router_event).await;
    router_output
}

pub(crate) fn activation_snapshot_from_router_output(
    router_output: &RouterOutput,
) -> ActivationSnapshot {
    ActivationSnapshot {
        concepts: router_output.concepts.clone(),
        hard_triggers: router_output.hard_triggers.clone(),
        soft_recommendations: router_output.soft_recommendations.clone(),
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
    let runs = activation_snapshot
        .hard_triggers
        .iter()
        .filter_map(|module_name| {
            module_instructions
                .get(module_name)
                .map(|instructions| (module_name.clone(), instructions.clone()))
        })
        .map(|(module_name, instructions)| async move {
            let result = execute_submodule(
                &module_name,
                activation_snapshot,
                &instructions,
                Some("hard_trigger"),
            )
            .await;
            (module_name, result)
        })
        .collect::<Vec<_>>();
    join_all(runs)
        .await
        .into_iter()
        .map(|(module, result)| match result {
            Ok(text) => HardTriggerResult { module, text },
            Err(err) => HardTriggerResult {
                module,
                text: format!("error: {}", err),
            },
        })
        .collect()
}

async fn infer_activation_query_terms(
    input_text: &str,
    active_module_names: &[String],
    modules: &Modules,
    state: &AppState,
) -> Vec<String> {
    let fallback = build_activation_query_terms(input_text);
    if input_text.trim().is_empty() {
        return fallback;
    }

    let module_lines = if active_module_names.is_empty() {
        "none".to_string()
    } else {
        active_module_names
            .iter()
            .map(|name| format!("- {}", name))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let context = format!(
        "User input:\n{}\n\nActive submodules:\n{}\n\nReturn only compact JSON:\n{{\"activation_query_terms\":[\"...\"]}}\nRules:\n- 1 to {} terms\n- short lookup-oriented terms\n- no explanations",
        input_text, module_lines, ROUTER_QUERY_TERMS_MAX
    );
    let instructions = format!(
        "{}\n\n{}",
        modules.runtime.base_instructions,
        "You are the router. Extract activation query terms for concept-graph lookup only."
    );
    let adapter = ResponseApiAdapter::new(build_router_config(instructions, &modules.runtime));
    let response = match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => response,
        Err(err) => {
            let detail = err.to_string();
            emit_router_debug_error(state, &context, &detail).await;
            return fallback;
        }
    };
    emit_router_debug_raw(state, &context, &response.raw, &response.text).await;

    let mut terms = parse_router_query_terms(&response.text);
    if terms.is_empty() {
        return fallback;
    }
    if terms.len() > ROUTER_QUERY_TERMS_MAX {
        terms.truncate(ROUTER_QUERY_TERMS_MAX);
    }
    terms
}

fn build_activation_query_terms(input_text: &str) -> Vec<String> {
    let normalized = input_text.trim().to_lowercase();
    if normalized.is_empty() {
        return Vec::new();
    }
    let mut terms = tokenize(&normalized);
    terms.push(normalized);
    terms.sort();
    terms.dedup();
    terms
}

fn compute_module_scores_minimal(
    input_text: &str,
    activation_query_terms: &[String],
    activation_concepts: &[String],
    active_module_names: &[String],
) -> Vec<(String, f32)> {
    let lower = input_text.to_lowercase();
    let query_terms = activation_query_terms
        .iter()
        .map(|value| value.to_lowercase())
        .collect::<HashSet<_>>();
    let concept_terms = activation_concepts
        .iter()
        .map(|value| value.to_lowercase())
        .collect::<Vec<_>>();
    let mut scored = Vec::<(String, f32)>::new();
    for name in active_module_names {
        let name_lc = name.to_lowercase();
        let matched_input = lower.contains(name_lc.as_str());
        let matched_query = query_terms.contains(name_lc.as_str());
        let matched_concept = concept_terms
            .iter()
            .any(|concept| concept == &name_lc || concept.contains(name_lc.as_str()));
        let score = if matched_input || matched_query || matched_concept {
            1.0
        } else {
            0.0
        };
        scored.push((name.clone(), score));
    }
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored
}

fn select_modules_by_threshold(scores: &[(String, f32)], threshold: f32) -> Vec<String> {
    let threshold = threshold.clamp(0.0, 1.0);
    scores
        .iter()
        .filter(|(_, score)| *score >= threshold)
        .map(|(name, _)| name.clone())
        .collect()
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .map(|token| token.to_lowercase())
        .collect()
}

fn parse_router_query_terms(text: &str) -> Vec<String> {
    let parsed_json = serde_json::from_str::<serde_json::Value>(text).ok();
    let mut terms = Vec::new();
    if let Some(json) = parsed_json {
        if let Some(items) = json
            .get("activation_query_terms")
            .and_then(|value| value.as_array())
        {
            for item in items {
                if let Some(value) = item.as_str() {
                    terms.push(value.to_string());
                }
            }
        } else if let Some(items) = json.as_array() {
            for item in items {
                if let Some(value) = item.as_str() {
                    terms.push(value.to_string());
                }
            }
        }
    }
    if terms.is_empty() {
        terms = text
            .split([',', '\n', ';'])
            .map(str::trim)
            .map(|value| value.trim_matches(|c| c == '"' || c == '\'' || c == '-' || c == '*'))
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
    }
    let mut normalized = Vec::new();
    let mut seen = HashSet::<String>::new();
    for term in terms {
        let value = term.trim().to_lowercase();
        if value.is_empty() {
            continue;
        }
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

fn build_router_config(instructions: String, runtime: &ModuleRuntime) -> ResponseApiConfig {
    ResponseApiConfig {
        model: runtime.model.clone(),
        instructions,
        temperature: runtime.temperature,
        max_output_tokens: runtime.max_output_tokens,
        tools: Vec::new(),
        tool_handler: None,
        max_tool_rounds: 0,
    }
}

async fn emit_router_debug_raw(
    state: &AppState,
    context: &str,
    raw: &serde_json::Value,
    output_text: &str,
) {
    let event = build_event(
        "router",
        "text",
        json!({
            "raw": raw,
            "context": context,
            "output_text": output_text,
            "mode": "runtime",
        }),
        vec![
            "debug".to_string(),
            "llm.raw".to_string(),
            "mode:runtime".to_string(),
        ],
    );
    record_event(state, event).await;
}

async fn emit_router_debug_error(state: &AppState, context: &str, error: &str) {
    let event = build_event(
        "router",
        "text",
        json!({
            "mode": "runtime",
            "context": context,
            "error": error,
        }),
        vec![
            "debug".to_string(),
            "llm.error".to_string(),
            "error".to_string(),
            "mode:runtime".to_string(),
        ],
    );
    record_event(state, event).await;
}

async fn emit_concept_graph_query_event(
    state: &AppState,
    query_terms: &[String],
    limit: usize,
    result_concepts: &[String],
    error: Option<&str>,
) {
    let mut tags = vec!["debug".to_string(), "concept_graph.query".to_string()];
    let payload = if let Some(error) = error {
        tags.push("error".to_string());
        json!({
            "query_terms": query_terms,
            "limit": limit,
            "error": error,
        })
    } else {
        json!({
            "query_terms": query_terms,
            "limit": limit,
            "result_concepts": result_concepts,
        })
    };
    let event = build_event("router", "state", payload, tags);
    record_event(state, event).await;
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
}
