use axum::http::StatusCode;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
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

#[derive(Debug, Clone, Serialize)]
struct ConceptActivation {
    concept: String,
    score: f32,
}

#[derive(Debug, Clone, Serialize)]
struct RouterOutput {
    concept_activation: Vec<ConceptActivation>,
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

#[derive(Clone)]
struct DecisionToolHandler {
    state: AppState,
    input_text: String,
    router_output: RouterOutput,
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

async fn run_router(input_text: &str, modules: &Modules, state: &AppState) -> RouterOutput {
    let concept_activation = compute_concept_activation(input_text, state);
    let active_module_names = match modules.registry.list_active().await {
        Ok(defs) => defs.into_iter().map(|d| d.name).collect::<Vec<_>>(),
        Err(err) => {
            println!("MODULE_REGISTRY_ERROR error={}", err);
            Vec::new()
        }
    };
    let soft_recommendations = compute_soft_recommendations(
        input_text,
        &concept_activation,
        &active_module_names,
        state.router.recommendation_threshold,
    );
    let router_output = RouterOutput {
        concept_activation,
        soft_recommendations,
    };
    let router_event = build_event(
        "internal",
        "state",
        serde_json::to_value(&router_output)
            .unwrap_or_else(|_| json!({ "error": "router_output_serialize_failed" })),
        vec!["router".to_string()],
    );
    record_event(state, router_event).await;
    router_output
}

fn compute_concept_activation(input_text: &str, state: &AppState) -> Vec<ConceptActivation> {
    let normalized = input_text.trim().to_lowercase();
    if normalized.is_empty() {
        return Vec::new();
    }
    let tokens = tokenize(&normalized);
    let mut score_map = BTreeMap::<String, f32>::new();
    let search_limit = state.router.concept_top_n.saturating_mul(4).max(8);
    let mut queries = tokens.clone();
    queries.push(normalized.clone());
    queries.dedup();
    for query in queries {
        for record in state.state_store.search(&query, search_limit) {
            let key = record.key.trim().to_string();
            if key.is_empty() {
                continue;
            }
            let key_lc = key.to_lowercase();
            let content_lc = record.content.to_lowercase();
            let mut score = 0.0f32;
            if key_lc.contains(&normalized) {
                score += 0.65;
            }
            if content_lc.contains(&normalized) {
                score += 0.25;
            }
            for token in &tokens {
                if key_lc.contains(token) {
                    score += 0.2;
                }
                if content_lc.contains(token) {
                    score += 0.1;
                }
            }
            if score <= 0.0 {
                score = 0.05;
            }
            let entry = score_map.entry(key).or_insert(0.0);
            *entry = (*entry).max(score.min(1.0));
        }
    }
    let mut scored = score_map
        .into_iter()
        .map(|(concept, score)| ConceptActivation { concept, score })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.concept.cmp(&b.concept))
    });
    scored.truncate(state.router.concept_top_n.max(1));
    scored
}

fn compute_soft_recommendations(
    input_text: &str,
    concept_activation: &[ConceptActivation],
    active_module_names: &[String],
    threshold: f32,
) -> Vec<String> {
    let lower = input_text.to_lowercase();
    let token_set = tokenize(&lower).into_iter().collect::<HashSet<_>>();
    let concept_text = concept_activation
        .iter()
        .map(|value| value.concept.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let threshold = threshold.clamp(0.0, 1.0);
    let mut scored = Vec::<(String, f32)>::new();
    for name in active_module_names {
        let mut score = 0.0f32;
        let name_lc = name.to_lowercase();
        if token_set.contains(name_lc.as_str()) || lower.contains(name_lc.as_str()) {
            score += 0.8;
        }
        if concept_text.contains(name_lc.as_str()) {
            score += 0.4;
        }
        for (keyword, delta) in module_keywords(name) {
            if lower.contains(keyword) || token_set.contains(keyword) {
                score += delta;
            }
        }
        if score >= threshold {
            scored.push((name.clone(), score.min(1.0)));
        }
    }
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.into_iter().map(|(name, _)| name).collect()
}

fn module_keywords(name: &str) -> Vec<(&'static str, f32)> {
    match name {
        "curiosity" => vec![
            ("why", 0.25),
            ("how", 0.2),
            ("learn", 0.2),
            ("explore", 0.2),
            ("curious", 0.2),
            ("なぜ", 0.25),
            ("どうして", 0.25),
            ("知りたい", 0.2),
            ("気になる", 0.2),
        ],
        "self_preservation" => vec![
            ("risk", 0.25),
            ("safe", 0.25),
            ("danger", 0.25),
            ("error", 0.2),
            ("security", 0.2),
            ("危険", 0.25),
            ("不安", 0.2),
            ("安全", 0.25),
            ("失敗", 0.2),
        ],
        "social_approval" => vec![
            ("thanks", 0.2),
            ("thank", 0.2),
            ("sorry", 0.2),
            ("help", 0.2),
            ("ありがとう", 0.25),
            ("ごめん", 0.2),
            ("嬉しい", 0.2),
            ("助けて", 0.2),
        ],
        _ => Vec::new(),
    }
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
    let handler = DecisionToolHandler {
        state: state.clone(),
        input_text: input_text.to_string(),
        router_output: router_output.clone(),
        base_handler: modules.runtime.tool_handler.clone(),
        module_instructions: module_instructions.clone(),
    };
    let adapter = ResponseApiAdapter::new(build_config_with_tools_and_handler(
        compose_instructions(&base_instructions, &decision_instructions),
        &modules.runtime,
        decision_tools(&modules.runtime.tools, module_instructions.keys().cloned()),
        Arc::new(handler),
    ));
    let context = format!(
        "Latest user input: {}\nConcept activation(top {}):\n{}\nSubmodule recommendations:\n{}\nRecent event history:\n{}\nReturn: decision=<respond|ignore|question> reason=<short> question=<text|none>.",
        input_text,
        state.router.concept_top_n.max(1),
        format_concept_activation(&router_output.concept_activation),
        format_soft_recommendations(&router_output.soft_recommendations),
        history
    );
    println!(
        "MODULE_INPUT name=decision role=decision bytes={}",
        context.len()
    );

    let response = match adapter.respond(LlmRequest { input: context }).await {
        Ok(response) => response,
        Err(err) => {
            let error_text = format!("error: {}", err);
            let error_event = build_event(
                "internal",
                "text",
                json!({ "text": error_text }),
                vec!["decision".to_string(), "error".to_string()],
            );
            record_event(state, error_event).await;
            return ModuleOutput {
                text: format!("error: {}", err),
            };
        }
    };

    let parsed = parse_decision(&response.text);
    let reason_text = parsed.reason.unwrap_or_else(|| "none".to_string());
    let decision_event = build_event(
        "internal",
        "text",
        json!({ "text": format!("decision={} reason={}", parsed.decision, reason_text) }),
        vec!["decision".to_string()],
    );
    record_event(state, decision_event.clone()).await;

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
    let handler = DecisionToolHandler {
        state: state.clone(),
        input_text: input_text.to_string(),
        router_output: router_output.clone(),
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
        format!(
            "Latest user input: {}\nConcept activation(top {}):\n{}\nSubmodule recommendations:\n{}\nRecent event history:\n{}\nReturn: decision=<respond|ignore|question> reason=<short> question=<text|none>.",
            input_text,
            state.router.concept_top_n.max(1),
            format_concept_activation(&router_output.concept_activation),
            format_soft_recommendations(&router_output.soft_recommendations),
            history
        )
    });
    let response = adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let parsed = parse_decision(&response.text);
    let reason_text = parsed.reason.unwrap_or_else(|| "none".to_string());
    let decision_event = build_event(
        "internal",
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
    let response = adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let response_event = build_event(
        "internal",
        "text",
        json!({ "text": response.text.clone() }),
        vec!["submodule".to_string(), format!("module:{}", name)],
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
                "internal",
                "text",
                json!({ "text": text }),
                vec!["submodule".to_string(), format!("module:{}", name)],
            )
        })
        .collect::<Vec<_>>();
    events.splice(insert_index..insert_index, synthetic.drain(..));
}

fn event_submodule_name(event: &Event) -> Option<&str> {
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
        "debug",
        "text",
        json!({
            "raw": response.raw.clone(),
            "context": context,
            "output_text": response.text.clone(),
            "module": module,
            "mode": mode,
        }),
        vec![
            "debug".to_string(),
            "llm.raw".to_string(),
            format!("module:{}", module),
            format!("mode:{}", mode),
        ],
    );
    record_event(state, raw_event).await;
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
                    &self.router_output,
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
    router_output: &RouterOutput,
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
        format_concept_activation(&router_output.concept_activation),
        format_soft_recommendations(&router_output.soft_recommendations),
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

    match adapter.respond(LlmRequest { input }).await {
        Ok(response) => {
            let response_event = build_event(
                "internal",
                "text",
                json!({ "text": response.text }),
                vec![role_tag.to_string(), format!("module:{}", name)],
            );
            record_event(state, response_event).await;
            ModuleOutput {
                text: response.text,
            }
        }
        Err(err) => {
            let error_text = format!("error: {}", err);
            let error_event = build_event(
                "internal",
                "text",
                json!({ "text": error_text }),
                vec![
                    role_tag.to_string(),
                    format!("module:{}", name),
                    "error".to_string(),
                ],
            );
            record_event(state, error_event).await;
            ModuleOutput {
                text: format!("error: {}", err),
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
                "required": [],
                "additionalProperties": false
            })),
            strict: Some(true),
        }));
    }
    tools
}

fn format_concept_activation(concepts: &[ConceptActivation]) -> String {
    if concepts.is_empty() {
        return "none".to_string();
    }
    concepts
        .iter()
        .map(|value| format!("- {}: {:.3}", value.concept, value.score))
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
                "internal",
                "text",
                json!({ "text": "old curiosity" }),
                vec!["submodule".to_string(), "module:curiosity".to_string()],
            ),
            build_event(
                "internal",
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
            .find(|event| event.meta.tags.iter().any(|tag| tag == "module:curiosity"))
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
            .find(|event| {
                event
                    .meta
                    .tags
                    .iter()
                    .any(|tag| tag == "module:social_approval")
            })
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
}
