use futures::future::join_all;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
};

use crate::event::build_event;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig, ToolError};
use crate::prompts::PromptOverrides;
use crate::tools::concept_graph_tools;
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
    pub(crate) active_concepts_from_concept_graph: Vec<String>,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
    pub(crate) hard_trigger_results: Vec<HardTriggerResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivationSnapshot {
    pub(crate) active_concepts_from_concept_graph: Vec<String>,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
}

pub(crate) async fn run_router<F, Fut>(
    input_text: &str,
    module_instructions: &HashMap<String, String>,
    modules: &Modules,
    state: &AppState,
    overrides: &PromptOverrides,
    execute_submodule: F,
) -> RouterOutput
where
    F: Fn(&str, &ActivationSnapshot, &str, Option<&str>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolError>> + Send,
{
    let active_module_names = module_instructions.keys().cloned().collect::<Vec<_>>();
    let activation_query_terms =
        infer_activation_query_terms(input_text, &active_module_names, modules, state, overrides)
            .await;
    let concept_limit = state.router.concept_top_n.max(1);
    let active_concepts_from_concept_graph = resolve_active_concepts_from_concept_graph(
        input_text,
        &activation_query_terms,
        concept_limit,
        modules,
        state,
        overrides,
    )
    .await;
    emit_concept_graph_query_event(
        state,
        &activation_query_terms,
        concept_limit,
        &active_concepts_from_concept_graph,
    )
    .await;

    let scores = compute_module_scores_minimal(
        input_text,
        &activation_query_terms,
        &active_concepts_from_concept_graph,
        &active_module_names,
    );
    let hard_triggers = select_modules_by_threshold(&scores, state.router.hard_trigger_threshold);
    let soft_recommendations =
        select_modules_by_threshold(&scores, state.router.recommendation_threshold);
    let activation_snapshot = ActivationSnapshot {
        active_concepts_from_concept_graph: active_concepts_from_concept_graph.clone(),
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
        active_concepts_from_concept_graph,
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
        active_concepts_from_concept_graph: router_output.active_concepts_from_concept_graph.clone(),
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
    overrides: &PromptOverrides,
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
    let context = render_router_context_template(
        &state.input.router_context_template,
        input_text,
        &module_lines,
        ROUTER_QUERY_TERMS_MAX,
    );
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| modules.runtime.base_instructions.clone());
    let router_instructions = overrides
        .router
        .clone()
        .unwrap_or_else(|| state.router_instructions.clone());
    let instructions = format!(
        "{}\n\n{}",
        base_instructions, router_instructions
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

fn render_router_context_template(
    template: &str,
    latest_user_input: &str,
    active_submodules: &str,
    router_query_terms_max: usize,
) -> String {
    template
        .replace("{{latest_user_input}}", latest_user_input)
        .replace("{{active_submodules}}", active_submodules)
        .replace(
            "{{router_query_terms_max}}",
            &router_query_terms_max.to_string(),
        )
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
        tools: concept_graph_tools(true, true),
        tool_handler: Some(runtime.tool_handler.clone()),
        max_tool_rounds: runtime.max_tool_rounds,
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
    active_concepts_from_concept_graph: &[String],
) {
    let event = build_event(
        "router",
        "state",
        json!({
            "query_terms": query_terms,
            "limit": limit,
            "active_concepts_from_concept_graph": active_concepts_from_concept_graph,
        }),
        vec!["debug".to_string(), "concept_graph.query".to_string()],
    );
    record_event(state, event).await;
}

async fn resolve_active_concepts_from_concept_graph(
    input_text: &str,
    activation_query_terms: &[String],
    concept_limit: usize,
    modules: &Modules,
    state: &AppState,
    overrides: &PromptOverrides,
) -> Vec<String> {
    let query_terms_text = if activation_query_terms.is_empty() {
        "none".to_string()
    } else {
        activation_query_terms.join(", ")
    };
    let context = format!(
        "latest_user_input:\n{}\n\nactivation_query_terms:\n{}\n\nconcept_limit:\n{}\n\nUse concept_search only as intermediate lookup for ambiguity absorption. Use recall_query to produce the final active concepts state. Return only compact JSON: {{\"active_concepts_from_concept_graph\":[\"...\"]}}",
        input_text, query_terms_text, concept_limit
    );
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| modules.runtime.base_instructions.clone());
    let router_instructions = overrides
        .router
        .clone()
        .unwrap_or_else(|| state.router_instructions.clone());
    let instructions = format!(
        "{}\n\n{}\n\nYou are the router preconscious module. You must call concept_search first, then use its results only to make recall_query calls. Do not output concept_search intermediate results.",
        base_instructions, router_instructions
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
            return Vec::new();
        }
    };
    emit_router_debug_raw(state, &context, &response.raw, &response.text).await;
    parse_active_concepts_from_router_output(&response.text)
}

fn parse_active_concepts_from_router_output(text: &str) -> Vec<String> {
    let parsed_json = serde_json::from_str::<serde_json::Value>(text).ok();
    let mut values = Vec::new();
    if let Some(json) = parsed_json {
        if let Some(items) = json
            .get("active_concepts_from_concept_graph")
            .and_then(|value| value.as_array())
        {
            for item in items {
                if let Some(value) = item.as_str() {
                    values.push(value.to_string());
                }
            }
        } else if let Some(items) = json.as_array() {
            for item in items {
                if let Some(value) = item.as_str() {
                    values.push(value.to_string());
                }
            }
        }
    }
    if values.is_empty() {
        values = text
            .split([',', '\n', ';'])
            .map(str::trim)
            .map(|value| value.trim_matches(|c| c == '"' || c == '\'' || c == '-' || c == '*'))
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
    }
    let mut normalized = Vec::new();
    let mut seen = HashSet::<String>::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            normalized.push(trimmed.to_string());
        }
    }
    normalized
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
