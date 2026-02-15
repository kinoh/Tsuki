use axum::http::StatusCode;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};
use tokio::runtime::Handle;

use crate::event::{build_event, Event};
use crate::llm::{
    LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig, ToolError, ToolHandler,
};
use crate::module_registry::ModuleRegistryReader;
use crate::prompts::PromptOverrides;
use crate::{record_event, AppState, DebugRunRequest, DebugRunResponse, ModuleRuntime, Modules};

const SUBMODULE_TOOL_PREFIX: &str = "run_submodule__";

#[derive(Debug, Deserialize)]
struct InputMessage {
    #[serde(default, rename = "type")]
    kind: String,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppendInputMode {
    AlwaysNew,
    ReuseOpen,
}

impl AppendInputMode {
    fn from_request(value: Option<&str>) -> Self {
        match value {
            Some(raw) if raw.eq_ignore_ascii_case("reuse_open") => Self::ReuseOpen,
            _ => Self::AlwaysNew,
        }
    }
}

#[derive(Debug, Clone)]
struct ModuleOutput {
    text: String,
}

#[derive(Debug, Clone)]
struct HardTriggerResult {
    module: String,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct RouterOutput {
    activation_query_terms: Vec<String>,
}

#[derive(Debug, Clone)]
struct ActivationSnapshot {
    concepts: Vec<String>,
    hard_triggers: Vec<String>,
    soft_recommendations: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SubmoduleToolArgs {
    #[serde(default)]
    focus: Option<String>,
}

struct DecisionParsed {
    decision: String,
    reason: Option<String>,
}

struct DecisionContextTemplateVars<'a> {
    latest_user_input: &'a str,
    concept_top_n: usize,
    active_concepts_from_concept_graph: &'a str,
    outputs_from_immediately_executed_submodules: &'a str,
    candidate_submodules_by_interest_match: &'a str,
    recent_event_history: &'a str,
}

#[derive(Clone)]
struct DecisionToolHandler {
    state: AppState,
    input_text: String,
    activation_snapshot: ActivationSnapshot,
    base_handler: Arc<dyn ToolHandler>,
    module_instructions: HashMap<String, String>,
}

pub(crate) async fn run_debug_module(
    state: &AppState,
    name: String,
    payload: DebugRunRequest,
) -> Result<DebugRunResponse, (StatusCode, String)> {
    let context_override = payload
        .context_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if payload.input.trim().is_empty() && context_override.is_none() {
        return Err((StatusCode::BAD_REQUEST, "input is required".to_string()));
    }
    if context_override.is_some() && name == "submodules" {
        return Err((
            StatusCode::BAD_REQUEST,
            "context_override is not supported for submodules".to_string(),
        ));
    }
    let include_history = payload.include_history.unwrap_or(true);
    let history_cutoff_ts = payload.history_cutoff_ts.as_deref();
    let excluded_event_ids = payload
        .exclude_event_ids
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    let append_mode = AppendInputMode::from_request(payload.append_input_mode.as_deref());
    if context_override.is_none() {
        maybe_append_debug_input_event(
            state,
            payload.input.trim(),
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            append_mode,
        )
        .await;
    }
    let output = if name == "decision" {
        run_decision_debug(
            &payload.input,
            context_override,
            payload.submodule_outputs.as_deref(),
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            state,
        )
        .await?
    } else if name == "submodules" {
        run_all_submodules_debug(
            &payload.input,
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            state,
        )
        .await?
    } else {
        run_submodule_debug(
            &name,
            &payload.input,
            context_override,
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            state,
        )
        .await?
    };
    Ok(DebugRunResponse { output })
}

pub(crate) async fn handle_input(raw: String, state: &AppState) {
    let parsed: Result<InputMessage, _> = serde_json::from_str(&raw);
    let input = match parsed {
        Ok(message) => message,
        Err(_) => {
            let event = build_event(
                "system",
                "text",
                json!({ "text": "invalid input payload" }),
                vec!["error".to_string()],
            );
            record_event(state, event).await;
            return;
        }
    };

    let kind = if input.kind.trim().is_empty() {
        "message".to_string()
    } else {
        input.kind.trim().to_string()
    };

    if kind != "message" && kind != "sensory" {
        let event = build_event(
            "system",
            "text",
            json!({ "text": "invalid input type" }),
            vec!["error".to_string()],
        );
        record_event(state, event).await;
        return;
    }

    let input_event = build_event(
        "user",
        "text",
        json!({ "text": input.text }),
        vec!["input".to_string(), format!("type:{}", kind)],
    );
    record_event(state, input_event.clone()).await;

    let input_text = input_event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let router_output = run_router(&input_text, &state.modules, state).await;
    let _decision_output = run_decision(&input_text, &router_output, &state.modules, state).await;
}

async fn maybe_append_debug_input_event(
    state: &AppState,
    input_text: &str,
    include_history: bool,
    cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    append_mode: AppendInputMode,
) {
    let normalized_input = input_text.trim();
    if normalized_input.is_empty() {
        return;
    }
    let should_append = match append_mode {
        AppendInputMode::AlwaysNew => true,
        AppendInputMode::ReuseOpen => {
            if !include_history {
                true
            } else {
                let events = latest_events(state, 1000, cutoff_ts, Some(excluded_event_ids)).await;
                should_append_debug_input_for_reuse_open(normalized_input, &events)
            }
        }
    };
    if !should_append {
        return;
    }
    let event = build_event(
        "user",
        "text",
        json!({ "text": normalized_input }),
        vec!["input".to_string(), "type:message".to_string()],
    );
    record_event(state, event).await;
}

fn should_append_debug_input_for_reuse_open(input_text: &str, events: &[Event]) -> bool {
    let mut saw_decision_after_input = false;
    for event in events {
        if is_decision_event(event) {
            saw_decision_after_input = true;
            continue;
        }
        if !is_user_input_event(event) {
            continue;
        }
        if saw_decision_after_input {
            return true;
        }
        let previous_input = event
            .payload
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim();
        return previous_input != input_text;
    }
    true
}

fn is_user_input_event(event: &Event) -> bool {
    event.source == "user" && event.meta.tags.iter().any(|tag| tag == "input")
}

fn is_decision_event(event: &Event) -> bool {
    event.meta.tags.iter().any(|tag| tag == "decision")
}

async fn run_router(input_text: &str, _modules: &Modules, state: &AppState) -> RouterOutput {
    let router_output = RouterOutput {
        activation_query_terms: build_activation_query_terms(input_text),
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

async fn build_activation_snapshot(
    input_text: &str,
    router_output: &RouterOutput,
    active_module_names: &[String],
    state: &AppState,
) -> ActivationSnapshot {
    let concept_limit = state.router.concept_top_n.max(1);
    let query_terms = router_output.activation_query_terms.clone();
    let concepts = match state
        .activation_concept_graph
        .concept_search(&query_terms, concept_limit)
        .await
    {
        Ok(values) => {
            emit_concept_graph_query_event(state, &query_terms, concept_limit, &values, None).await;
            values
        }
        Err(err) => {
            println!(
                "ACTIVATION_CONCEPT_GRAPH_ERROR op=concept_search error={}",
                err
            );
            let error = err.to_string();
            emit_concept_graph_query_event(state, &query_terms, concept_limit, &[], Some(&error))
                .await;
            Vec::new()
        }
    };
    let scores = compute_module_scores_minimal(
        input_text,
        &router_output.activation_query_terms,
        &concepts,
        active_module_names,
    );
    let hard_triggers = select_modules_by_threshold(&scores, state.router.hard_trigger_threshold);
    let soft_recommendations =
        select_modules_by_threshold(&scores, state.router.recommendation_threshold);
    ActivationSnapshot {
        concepts,
        hard_triggers,
        soft_recommendations,
    }
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

async fn run_decision(
    input_text: &str,
    router_output: &RouterOutput,
    modules: &Modules,
    state: &AppState,
) -> ModuleOutput {
    let history = format_event_history(state, state.limits.decision_history, None, None).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| modules.runtime.base_instructions.clone());
    let decision_instructions = overrides
        .decision
        .clone()
        .unwrap_or_else(|| state.decision_instructions.clone());
    let module_defs = match modules.registry.list_active().await {
        Ok(list) => list,
        Err(err) => {
            println!("MODULE_REGISTRY_ERROR error={}", err);
            Vec::new()
        }
    };
    let active_module_names = module_defs
        .iter()
        .map(|definition| definition.name.clone())
        .collect::<Vec<_>>();
    let mut module_instructions = HashMap::new();
    for definition in module_defs {
        let module_name = definition.name;
        let instructions = overrides
            .submodules
            .get(&module_name)
            .cloned()
            .unwrap_or(definition.instructions);
        module_instructions.insert(module_name, instructions);
    }
    let activation_snapshot =
        build_activation_snapshot(input_text, router_output, &active_module_names, state).await;
    let hard_trigger_results = run_hard_triggers(
        input_text,
        &activation_snapshot,
        &module_instructions,
        state,
    )
    .await;
    let handler = DecisionToolHandler {
        state: state.clone(),
        input_text: input_text.to_string(),
        activation_snapshot: activation_snapshot.clone(),
        base_handler: modules.runtime.tool_handler.clone(),
        module_instructions: module_instructions.clone(),
    };
    let adapter = ResponseApiAdapter::new(build_config_with_tools_and_handler(
        compose_instructions(&base_instructions, &decision_instructions),
        &modules.runtime,
        decision_tools(&modules.runtime.tools, module_instructions.keys().cloned()),
        Arc::new(handler),
    ));
    let concept_top_n = state.router.concept_top_n.max(1);
    let activation_concepts = format_activation_concepts(&activation_snapshot.concepts);
    let executed_submodule_outputs = format_hard_trigger_results(&hard_trigger_results);
    let submodule_candidates =
        format_soft_recommendations(&activation_snapshot.soft_recommendations);
    let context = render_decision_context_template(
        &state.input.decision_context_template,
        DecisionContextTemplateVars {
            latest_user_input: input_text,
            concept_top_n,
            active_concepts_from_concept_graph: &activation_concepts,
            outputs_from_immediately_executed_submodules: &executed_submodule_outputs,
            candidate_submodules_by_interest_match: &submodule_candidates,
            recent_event_history: &history,
        },
    );
    println!(
        "MODULE_INPUT name=decision role=decision bytes={}",
        context.len()
    );

    let response = match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => response,
        Err(err) => {
            let error_detail = err.to_string();
            let error_text = format!("error: {}", error_detail);
            let error_event = build_event(
                "decision",
                "text",
                json!({ "text": error_text }),
                vec!["decision".to_string(), "error".to_string()],
            );
            record_event(state, error_event).await;
            emit_debug_module_error_event(state, "decision", "runtime", &context, &error_detail)
                .await;
            return ModuleOutput {
                text: format!("error: {}", error_detail),
            };
        }
    };

    let parsed = parse_decision(&response.text);
    let reason_text = parsed.reason.unwrap_or_else(|| "none".to_string());
    let decision_event = build_event(
        "decision",
        "text",
        json!({ "text": format!("decision={} reason={}", parsed.decision, reason_text) }),
        vec!["decision".to_string()],
    );
    record_event(state, decision_event.clone()).await;
    emit_debug_module_events(state, "decision", "runtime", &context, &response).await;

    ModuleOutput {
        text: response.text,
    }
}

async fn run_decision_debug(
    input_text: &str,
    context_override: Option<&str>,
    submodule_outputs_raw: Option<&str>,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let router_output = run_router(input_text, &state.modules, state).await;
    let history = if context_override.is_some() {
        String::new()
    } else if include_history {
        format_decision_debug_history(
            state,
            state.limits.decision_history,
            history_cutoff_ts,
            Some(excluded_event_ids),
            submodule_outputs_raw,
        )
        .await
    } else {
        "none".to_string()
    };
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| state.modules.runtime.base_instructions.clone());
    let decision_instructions = overrides
        .decision
        .clone()
        .unwrap_or_else(|| state.decision_instructions.clone());
    let module_defs = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let active_module_names = module_defs
        .iter()
        .map(|definition| definition.name.clone())
        .collect::<Vec<_>>();
    let mut module_instructions = HashMap::new();
    for definition in module_defs {
        let module_name = definition.name;
        let instructions = overrides
            .submodules
            .get(&module_name)
            .cloned()
            .unwrap_or(definition.instructions);
        module_instructions.insert(module_name, instructions);
    }
    let activation_snapshot =
        build_activation_snapshot(input_text, &router_output, &active_module_names, state).await;
    let hard_trigger_results = run_hard_triggers(
        input_text,
        &activation_snapshot,
        &module_instructions,
        state,
    )
    .await;
    let handler = DecisionToolHandler {
        state: state.clone(),
        input_text: input_text.to_string(),
        activation_snapshot: activation_snapshot.clone(),
        base_handler: state.modules.runtime.tool_handler.clone(),
        module_instructions: module_instructions.clone(),
    };
    let adapter = ResponseApiAdapter::new(build_config_with_tools_and_handler(
        compose_instructions(&base_instructions, &decision_instructions),
        &state.modules.runtime,
        decision_tools(
            &state.modules.runtime.tools,
            module_instructions.keys().cloned(),
        ),
        Arc::new(handler),
    ));
    let context = context_override.map(str::to_string).unwrap_or_else(|| {
        let concept_top_n = state.router.concept_top_n.max(1);
        let activation_concepts = format_activation_concepts(&activation_snapshot.concepts);
        let executed_submodule_outputs = format_hard_trigger_results(&hard_trigger_results);
        let submodule_candidates =
            format_soft_recommendations(&activation_snapshot.soft_recommendations);
        render_decision_context_template(
            &state.input.decision_context_template,
            DecisionContextTemplateVars {
                latest_user_input: input_text,
                concept_top_n,
                active_concepts_from_concept_graph: &activation_concepts,
                outputs_from_immediately_executed_submodules: &executed_submodule_outputs,
                candidate_submodules_by_interest_match: &submodule_candidates,
                recent_event_history: &history,
            },
        )
    });
    let response = match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => response,
        Err(err) => {
            let error = err.to_string();
            emit_debug_module_error_event(state, "decision", "module_only", &context, &error).await;
            return Err((StatusCode::INTERNAL_SERVER_ERROR, error));
        }
    };
    let parsed = parse_decision(&response.text);
    let reason_text = parsed.reason.unwrap_or_else(|| "none".to_string());
    let decision_event = build_event(
        "decision",
        "text",
        json!({ "text": format!("decision={} reason={}", parsed.decision, reason_text) }),
        vec!["decision".to_string()],
    );
    record_event(state, decision_event.clone()).await;
    emit_debug_module_events(state, "decision", "module_only", &context, &response).await;
    Ok(response.text)
}

async fn run_all_submodules_debug(
    input_text: &str,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let module_names = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .into_iter()
        .map(|definition| definition.name)
        .collect::<Vec<_>>();
    if module_names.is_empty() {
        return Ok("no active submodules".to_string());
    }
    let runs = module_names.iter().map(|name| async move {
        let output = run_submodule_debug(
            name.as_str(),
            input_text,
            None,
            include_history,
            history_cutoff_ts,
            excluded_event_ids,
            state,
        )
        .await?;
        Ok::<(String, String), (StatusCode, String)>((name.clone(), output))
    });
    let mut outputs = Vec::with_capacity(module_names.len());
    for result in join_all(runs).await {
        let (name, output) = result?;
        outputs.push(format!("{}: {}", name, output));
    }
    Ok(outputs.join("\n"))
}

async fn run_submodule_debug(
    name: &str,
    input_text: &str,
    context_override: Option<&str>,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let history = if context_override.is_some() {
        String::new()
    } else if include_history {
        format_event_history(
            state,
            state.limits.submodule_history,
            history_cutoff_ts,
            Some(excluded_event_ids),
        )
        .await
    } else {
        "none".to_string()
    };
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| state.modules.runtime.base_instructions.clone());
    let module_defs = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let definition = module_defs
        .into_iter()
        .find(|definition| definition.name == name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "module not found".to_string()))?;
    let instructions = overrides
        .submodules
        .get(&definition.name)
        .cloned()
        .unwrap_or(definition.instructions);
    let adapter = ResponseApiAdapter::new(build_config(
        compose_instructions(&base_instructions, &instructions),
        &state.modules.runtime,
    ));
    let context = context_override
        .map(str::to_string)
        .unwrap_or_else(|| format!("User input: {}\nRecent events:\n{}", input_text, history));
    let response = match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => response,
        Err(err) => {
            let error = err.to_string();
            emit_debug_module_error_event(state, name, "module_only", &context, &error).await;
            return Err((StatusCode::INTERNAL_SERVER_ERROR, error));
        }
    };
    let response_event = build_event(
        format!("submodule:{}", name).as_str(),
        "text",
        json!({ "text": response.text.clone() }),
        vec!["submodule".to_string()],
    );
    record_event(state, response_event.clone()).await;
    emit_debug_module_events(state, name, "module_only", &context, &response).await;
    Ok(response.text)
}

fn parse_submodule_outputs(raw: Option<&str>) -> Vec<(String, String)> {
    let raw = match raw {
        Some(value) => value,
        None => return Vec::new(),
    };
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                return None;
            }
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

async fn format_decision_debug_history(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
    submodule_outputs_raw: Option<&str>,
) -> String {
    let mut events = latest_events(state, limit, cutoff_ts, excluded_event_ids).await;
    let submodule_overrides = parse_submodule_outputs(submodule_outputs_raw)
        .into_iter()
        .collect::<HashMap<_, _>>();
    if !submodule_overrides.is_empty() {
        apply_submodule_output_overrides(&mut events, &submodule_overrides);
    }
    if events.is_empty() {
        return "none".to_string();
    }
    let mut lines = Vec::with_capacity(events.len() + 1);
    lines.push("ts | role | message".to_string());
    lines.extend(events.iter().map(format_event_line));
    lines.join("\n")
}

fn apply_submodule_output_overrides(events: &mut Vec<Event>, overrides: &HashMap<String, String>) {
    let mut applied = HashSet::<String>::new();
    for event in events.iter_mut() {
        let Some(module_name) = event_submodule_name(event).map(str::to_string) else {
            continue;
        };
        let Some(override_text) = overrides.get(&module_name) else {
            continue;
        };
        event.payload = json!({ "text": override_text });
        applied.insert(module_name);
    }
    let missing = overrides
        .iter()
        .filter(|(name, _)| !applied.contains(name.as_str()))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }
    let insert_index = events
        .iter()
        .rposition(is_user_input_event)
        .map(|index| index + 1)
        .unwrap_or(events.len());
    let mut synthetic = missing
        .into_iter()
        .map(|(name, text)| {
            build_event(
                format!("submodule:{}", name).as_str(),
                "text",
                json!({ "text": text }),
                vec!["submodule".to_string()],
            )
        })
        .collect::<Vec<_>>();
    events.splice(insert_index..insert_index, synthetic.drain(..));
}

fn event_submodule_name(event: &Event) -> Option<&str> {
    if let Some(name) = event
        .source
        .strip_prefix("submodule:")
        .filter(|value| !value.is_empty())
    {
        return Some(name);
    }
    if !event.meta.tags.iter().any(|tag| tag == "submodule") {
        return None;
    }
    event
        .meta
        .tags
        .iter()
        .find_map(|tag| tag.strip_prefix("module:"))
        .filter(|value| !value.is_empty())
}

async fn emit_debug_module_events(
    state: &AppState,
    module: &str,
    mode: &str,
    context: &str,
    response: &crate::llm::LlmResponse,
) {
    let raw_event = build_event(
        module_owner_source(module).as_str(),
        "text",
        json!({
            "raw": response.raw.clone(),
            "context": context,
            "output_text": response.text.clone(),
            "mode": mode,
        }),
        vec![
            "debug".to_string(),
            "llm.raw".to_string(),
            format!("mode:{}", mode),
        ],
    );
    record_event(state, raw_event).await;
}

async fn emit_debug_module_error_event(
    state: &AppState,
    module: &str,
    mode: &str,
    context: &str,
    error: &str,
) {
    let error_event = build_event(
        module_owner_source(module).as_str(),
        "text",
        json!({
            "mode": mode,
            "context": context,
            "error": error,
        }),
        vec![
            "debug".to_string(),
            "llm.error".to_string(),
            "error".to_string(),
            format!("mode:{}", mode),
        ],
    );
    record_event(state, error_event).await;
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

impl ToolHandler for DecisionToolHandler {
    fn handle(&self, tool_name: &str, arguments: &str) -> Result<String, ToolError> {
        if let Some(module_name) = tool_name.strip_prefix(SUBMODULE_TOOL_PREFIX) {
            if !self.module_instructions.contains_key(module_name) {
                return Err(ToolError::new(format!(
                    "unknown submodule: {}",
                    module_name
                )));
            }
            let args: SubmoduleToolArgs = serde_json::from_str(arguments)
                .map_err(|err| ToolError::new(format!("invalid args: {}", err)))?;
            let output = tokio::task::block_in_place(|| {
                Handle::current().block_on(run_submodule_tool(
                    &self.state,
                    &self.input_text,
                    &self.activation_snapshot,
                    module_name,
                    self.module_instructions
                        .get(module_name)
                        .map(String::as_str)
                        .unwrap_or(""),
                    args.focus.as_deref(),
                ))
            })?;
            Ok(output)
        } else {
            self.base_handler.handle(tool_name, arguments)
        }
    }
}

async fn run_submodule_tool(
    state: &AppState,
    input_text: &str,
    activation_snapshot: &ActivationSnapshot,
    module_name: &str,
    module_instructions: &str,
    focus: Option<&str>,
) -> Result<String, ToolError> {
    let history = format_event_history(state, state.limits.submodule_history, None, None).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| state.modules.runtime.base_instructions.clone());
    let instructions = compose_instructions(&base_instructions, module_instructions);
    let adapter = ResponseApiAdapter::new(build_config(instructions, &state.modules.runtime));
    let context = format!(
        "Latest user input: {}\nConcept activation:\n{}\nSubmodule recommendations:\n{}\nRecent events:\n{}\nTool focus: {}",
        input_text,
        format_activation_concepts(&activation_snapshot.concepts),
        format_soft_recommendations(&activation_snapshot.soft_recommendations),
        history,
        focus.unwrap_or("none")
    );
    let output = run_module(
        state,
        module_name.to_string(),
        "submodule",
        Arc::new(adapter),
        context,
    )
    .await;
    Ok(output.text)
}

async fn run_hard_triggers(
    input_text: &str,
    activation_snapshot: &ActivationSnapshot,
    module_instructions: &HashMap<String, String>,
    state: &AppState,
) -> Vec<HardTriggerResult> {
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
            let result = run_submodule_tool(
                state,
                input_text,
                activation_snapshot,
                &module_name,
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

async fn run_module(
    state: &AppState,
    name: String,
    role_tag: &'static str,
    adapter: Arc<dyn LlmAdapter>,
    input: String,
) -> ModuleOutput {
    println!(
        "MODULE_INPUT name={} role={} bytes={}",
        name,
        role_tag,
        input.len()
    );

    let context = input;
    match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => {
            let response_event = build_event(
                owner_source_for_role(role_tag, &name).as_str(),
                "text",
                json!({ "text": response.text }),
                vec![role_tag.to_string()],
            );
            record_event(state, response_event).await;
            emit_debug_module_events(state, &name, "runtime", &context, &response).await;
            ModuleOutput {
                text: response.text,
            }
        }
        Err(err) => {
            let error_detail = err.to_string();
            let error_text = format!("error: {}", error_detail);
            let error_event = build_event(
                owner_source_for_role(role_tag, &name).as_str(),
                "text",
                json!({ "text": error_text }),
                vec![role_tag.to_string(), "error".to_string()],
            );
            record_event(state, error_event).await;
            emit_debug_module_error_event(state, &name, "runtime", &context, &error_detail).await;
            ModuleOutput {
                text: format!("error: {}", error_detail),
            }
        }
    }
}

async fn current_prompt_overrides(state: &AppState) -> PromptOverrides {
    state.prompts.read().await.clone()
}

async fn format_event_history(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
) -> String {
    let events = latest_events(state, limit, cutoff_ts, excluded_event_ids).await;
    if events.is_empty() {
        return "none".to_string();
    }
    let mut lines = Vec::with_capacity(events.len() + 1);
    lines.push("ts | role | message".to_string());
    lines.extend(events.iter().map(format_event_line));
    lines.join("\n")
}

async fn latest_events(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
) -> Vec<Event> {
    if limit == 0 {
        return Vec::new();
    }
    match state.event_store.latest(limit).await {
        Ok(events) => events
            .into_iter()
            .filter(|event| !is_debug_event(event))
            .filter(|event| {
                excluded_event_ids
                    .map(|ids| !ids.contains(event.event_id.as_str()))
                    .unwrap_or(true)
            })
            .filter(|event| {
                cutoff_ts
                    .map(|cutoff| event.ts.as_str() >= cutoff)
                    .unwrap_or(true)
            })
            .collect(),
        Err(err) => {
            println!("EVENT_STORE_ERROR error={}", err);
            Vec::new()
        }
    }
}

fn format_event_line(event: &Event) -> String {
    let role = event_role(event);
    let ts = format_local_ts_seconds(&event.ts);
    let payload_text = event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .map(|value| truncate(value, 160))
        .unwrap_or_else(|| truncate(&event.payload.to_string(), 160));
    format!("{} | {} | {}", ts, role, payload_text)
}

fn event_role(event: &Event) -> String {
    let tags = &event.meta.tags;
    if event.source == "user" {
        return "user".to_string();
    }
    if tags.iter().any(|tag| tag == "action") && tags.iter().any(|tag| tag == "response") {
        return "assistant".to_string();
    }
    if tags.iter().any(|tag| tag == "decision") {
        return "decision".to_string();
    }
    if let Some(module_name) = event
        .source
        .strip_prefix("submodule:")
        .filter(|value| !value.is_empty())
    {
        return format!("submodule:{}", module_name);
    }
    if tags.iter().any(|tag| tag == "submodule") {
        if let Some(module_name) = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("module:"))
            .filter(|value| !value.is_empty())
        {
            return format!("submodule:{}", module_name);
        }
        return "submodule".to_string();
    }
    event.source.clone()
}

fn format_local_ts_seconds(ts: &str) -> String {
    let parsed = match OffsetDateTime::parse(ts, &Rfc3339) {
        Ok(value) => value,
        Err(_) => return ts.to_string(),
    };
    let local = match UtcOffset::current_local_offset() {
        Ok(offset) => parsed.to_offset(offset),
        Err(_) => parsed,
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.year(),
        local.month() as u8,
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "…"
}

fn is_debug_event(event: &Event) -> bool {
    event.meta.tags.iter().any(|tag| tag == "debug")
}

fn module_owner_source(module: &str) -> String {
    if module.eq_ignore_ascii_case("decision") {
        return "decision".to_string();
    }
    format!("submodule:{}", module)
}

fn owner_source_for_role(role_tag: &str, module_name: &str) -> String {
    if role_tag == "decision" {
        return "decision".to_string();
    }
    if role_tag == "submodule" {
        return format!("submodule:{}", module_name);
    }
    "system".to_string()
}

fn parse_decision(text: &str) -> DecisionParsed {
    let decision = extract_field(text, "decision=", &["reason=", "question="])
        .and_then(|value| value.split_whitespace().next().map(|s| s.to_lowercase()))
        .unwrap_or_else(|| "respond".to_string());
    let reason = extract_field(text, "reason=", &["decision=", "question="]);
    let _question = extract_field(text, "question=", &["decision=", "reason="]).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    DecisionParsed { decision, reason }
}

fn extract_field(text: &str, key: &str, end_keys: &[&str]) -> Option<String> {
    let start = text.find(key)?;
    let after = &text[start + key.len()..];
    let mut end = after.len();
    for end_key in end_keys {
        if let Some(idx) = after.find(end_key) {
            end = end.min(idx);
        }
    }
    let value = after[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn render_decision_context_template(
    template: &str,
    vars: DecisionContextTemplateVars<'_>,
) -> String {
    template
        .replace("{{latest_user_input}}", vars.latest_user_input)
        .replace("{{concept_top_n}}", &vars.concept_top_n.to_string())
        .replace(
            "{{active_concepts_from_concept_graph}}",
            vars.active_concepts_from_concept_graph,
        )
        .replace(
            "{{outputs_from_immediately_executed_submodules}}",
            vars.outputs_from_immediately_executed_submodules,
        )
        .replace(
            "{{candidate_submodules_by_interest_match}}",
            vars.candidate_submodules_by_interest_match,
        )
        .replace("{{recent_event_history}}", vars.recent_event_history)
}

fn compose_instructions(base: &str, module_specific: &str) -> String {
    format!("{}\n\n{}", base, module_specific)
}

fn decision_tools(
    base_tools: &[async_openai::types::responses::Tool],
    module_names: impl IntoIterator<Item = String>,
) -> Vec<async_openai::types::responses::Tool> {
    use async_openai::types::responses::{FunctionTool, Tool};
    let mut tools = base_tools.to_vec();
    let mut names = module_names.into_iter().collect::<Vec<_>>();
    names.sort();
    for module_name in names {
        if module_name.trim().is_empty() {
            continue;
        }
        tools.push(Tool::Function(FunctionTool {
            name: format!("{}{}", SUBMODULE_TOOL_PREFIX, module_name),
            description: Some(format!(
                "Run submodule {} and return its recommendation text.",
                module_name
            )),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "focus": { "type": "string" }
                },
                "required": ["focus"],
                "additionalProperties": false
            })),
            strict: Some(true),
        }));
    }
    tools
}

fn format_activation_concepts(concepts: &[String]) -> String {
    if concepts.is_empty() {
        return "none".to_string();
    }
    concepts
        .iter()
        .map(|value| format!("- {}", value))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_hard_trigger_results(outputs: &[HardTriggerResult]) -> String {
    if outputs.is_empty() {
        return "none".to_string();
    }
    outputs
        .iter()
        .map(|output| format!("- {}: {}", output.module, truncate(&output.text, 160)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_soft_recommendations(recommendations: &[String]) -> String {
    if recommendations.is_empty() {
        return "none".to_string();
    }
    recommendations
        .iter()
        .map(|name| format!("- {}", name))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_config(instructions: String, runtime: &ModuleRuntime) -> ResponseApiConfig {
    ResponseApiConfig {
        model: runtime.model.clone(),
        instructions,
        temperature: runtime.temperature,
        max_output_tokens: runtime.max_output_tokens,
        tools: runtime.tools.clone(),
        tool_handler: Some(runtime.tool_handler.clone()),
        max_tool_rounds: runtime.max_tool_rounds,
    }
}

fn build_config_with_tools_and_handler(
    instructions: String,
    runtime: &ModuleRuntime,
    tools: Vec<async_openai::types::responses::Tool>,
    tool_handler: Arc<dyn ToolHandler>,
) -> ResponseApiConfig {
    ResponseApiConfig {
        model: runtime.model.clone(),
        instructions,
        temperature: runtime.temperature,
        max_output_tokens: runtime.max_output_tokens,
        tools,
        tool_handler: Some(tool_handler),
        max_tool_rounds: runtime.max_tool_rounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_submodule_output_overrides_replaces_and_inserts() {
        let mut events = vec![
            build_event(
                "user",
                "text",
                json!({ "text": "hello" }),
                vec!["input".to_string(), "type:message".to_string()],
            ),
            build_event(
                "submodule:curiosity",
                "text",
                json!({ "text": "old curiosity" }),
                vec!["submodule".to_string()],
            ),
            build_event(
                "decision",
                "text",
                json!({ "text": "decision=respond reason=test" }),
                vec!["decision".to_string()],
            ),
        ];
        let overrides = HashMap::from([
            ("curiosity".to_string(), "new curiosity".to_string()),
            ("social_approval".to_string(), "new social".to_string()),
        ]);
        apply_submodule_output_overrides(&mut events, &overrides);

        let curiosity = events
            .iter()
            .find(|event| event.source == "submodule:curiosity")
            .expect("curiosity event should exist");
        assert_eq!(
            curiosity
                .payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            "new curiosity"
        );

        let inserted = events
            .iter()
            .find(|event| event.source == "submodule:social_approval")
            .expect("social_approval event should be inserted");
        assert_eq!(
            inserted
                .payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            "new social"
        );
    }

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
