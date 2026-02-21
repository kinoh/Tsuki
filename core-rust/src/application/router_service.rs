use futures::future::join_all;
use serde::Serialize;
use serde_json::json;
use std::{
    collections::HashMap,
    future::Future,
    time::Instant,
};

use crate::event::build_event;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig, ToolError};
use crate::prompts::PromptOverrides;
use crate::tools::concept_graph_tools;
use crate::{record_event, AppState, ModuleRuntime, Modules};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HardTriggerResult {
    pub(crate) module: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RouterOutput {
    pub(crate) activation_query_terms: Vec<String>,
    pub(crate) active_concepts_from_concept_graph: String,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
    pub(crate) hard_trigger_results: Vec<HardTriggerResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivationSnapshot {
    pub(crate) active_concepts_from_concept_graph: String,
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
    let router_started = Instant::now();
    let active_module_names = module_instructions.keys().cloned().collect::<Vec<_>>();
    let concept_limit = state.router.query_terms_max.max(1);
    let active_concepts_from_concept_graph = resolve_active_concepts_from_concept_graph(
        input_text,
        &active_module_names,
        concept_limit,
        modules,
        state,
        overrides,
    )
    .await;
    let activation_query_terms = Vec::<String>::new();
    emit_concept_graph_query_event(
        state,
        &[],
        concept_limit,
        &active_concepts_from_concept_graph,
    )
    .await;

    let scores = compute_module_scores_minimal(input_text, &active_module_names);
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
    println!(
        "PERF router stage=end total_ms={} active_modules={} hard_triggers={} hard_results={} soft_recommendations={} concepts_len={}",
        router_started.elapsed().as_millis(),
        active_module_names.len(),
        router_output.hard_triggers.len(),
        router_output.hard_trigger_results.len(),
        router_output.soft_recommendations.len(),
        router_output.active_concepts_from_concept_graph.len()
    );
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
    active_module_names: &[String],
) -> Vec<(String, f32)> {
    let lower = input_text.to_lowercase();
    let mut scored = Vec::<(String, f32)>::new();
    for name in active_module_names {
        let name_lc = name.to_lowercase();
        let matched_input = lower.contains(name_lc.as_str());
        let score = if matched_input {
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
    tool_calls: &[crate::llm::ToolCallTrace],
) {
    let event = build_event(
        "router",
        "text",
        json!({
            "raw": raw,
            "context": context,
            "output_text": output_text,
            "tool_calls": tool_calls,
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
    active_concepts_from_concept_graph: &str,
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
    active_module_names: &[String],
    concept_limit: usize,
    modules: &Modules,
    state: &AppState,
    overrides: &PromptOverrides,
) -> String {
    let started = Instant::now();
    let module_lines = if active_module_names.is_empty() {
        "none".to_string()
    } else {
        active_module_names
            .iter()
            .map(|name| format!("- {}", name))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let base_context = render_router_context_template(
        &state.input.router_context_template,
        input_text,
        &module_lines,
        state.router.query_terms_max.max(1),
    );
    let context = format!("{}\n\nconcept_limit:\n{}", base_context, concept_limit);
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
    let llm_started = Instant::now();
    let response = match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => {
            println!(
                "PERF router.llm stage=respond ms={} ok=true output_len={} tool_calls={}",
                llm_started.elapsed().as_millis(),
                response.text.len(),
                response.tool_calls.len()
            );
            response
        }
        Err(err) => {
            let detail = err.to_string();
            println!(
                "PERF router.llm stage=respond ms={} ok=false error={}",
                llm_started.elapsed().as_millis(),
                detail
            );
            emit_router_debug_error(state, &context, &detail).await;
            return "none".to_string();
        }
    };
    emit_router_debug_raw(
        state,
        &context,
        &response.raw,
        &response.text,
        &response.tool_calls,
    )
    .await;
    let trimmed = response.text.trim();
    if trimmed.is_empty() {
        println!(
            "PERF router stage=resolve_concepts total_ms={} output=none_empty",
            started.elapsed().as_millis()
        );
        "none".to_string()
    } else {
        println!(
            "PERF router stage=resolve_concepts total_ms={} output_len={}",
            started.elapsed().as_millis(),
            trimmed.len()
        );
        trimmed.to_string()
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

}
