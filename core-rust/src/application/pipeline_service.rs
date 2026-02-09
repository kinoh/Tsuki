use axum::http::StatusCode;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashSet, sync::Arc};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

use crate::event::{build_event, Event};
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig};
use crate::module_registry::ModuleRegistryReader;
use crate::prompts::PromptOverrides;
use crate::{record_event, AppState, DebugRunRequest, DebugRunResponse, Modules, ModuleRuntime};

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
    name: String,
    text: String,
}

struct DecisionParsed {
    decision: String,
    reason: Option<String>,
    question: Option<String>,
}

pub(crate) async fn run_debug_module(
    state: &AppState,
    name: String,
    payload: DebugRunRequest,
) -> Result<DebugRunResponse, (StatusCode, String)> {
    if payload.input.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "input is required".to_string()));
    }
    let include_history = payload.include_history.unwrap_or(true);
    let history_cutoff_ts = payload.history_cutoff_ts.as_deref();
    let excluded_event_ids = payload
        .exclude_event_ids
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    let append_mode = AppendInputMode::from_request(payload.append_input_mode.as_deref());
    maybe_append_debug_input_event(
        state,
        payload.input.trim(),
        include_history,
        history_cutoff_ts,
        &excluded_event_ids,
        append_mode,
    )
    .await;
    let output = if name == "decision" {
        run_decision_debug(
            &payload.input,
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

    let _submodule_outputs = run_submodules(&input_text, &state.modules, state).await;
    let _decision_output = run_decision(&input_text, &state.modules, state).await;
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
    record_event(state, event.clone()).await;
    emit_debug_input_worklog(state, normalized_input, &event).await;
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

async fn run_submodules(input_text: &str, modules: &Modules, state: &AppState) -> Vec<ModuleOutput> {
    let history = format_event_history(state, state.limits.submodule_history, None, None).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| modules.runtime.base_instructions.clone());
    let module_defs = match modules.registry.list_active().await {
        Ok(list) => list,
        Err(err) => {
            println!("MODULE_REGISTRY_ERROR error={}", err);
            Vec::new()
        }
    };
    let tasks = module_defs
        .into_iter()
        .map(|definition| {
            let input = format!("User input: {}\nRecent events:\n{}", input_text, history);
            let instructions = overrides
                .submodules
                .get(&definition.name)
                .cloned()
                .unwrap_or_else(|| definition.instructions.clone());
            let instructions = compose_instructions(&base_instructions, &instructions);
            let adapter = ResponseApiAdapter::new(build_config(instructions, &modules.runtime));
            run_module(
                state,
                definition.name.clone(),
                "submodule",
                Arc::new(adapter),
                input,
            )
        })
        .collect::<Vec<_>>();

    join_all(tasks).await
}

async fn run_decision(input_text: &str, modules: &Modules, state: &AppState) -> ModuleOutput {
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
    let adapter = ResponseApiAdapter::new(build_config(
        compose_instructions(&base_instructions, &decision_instructions),
        &modules.runtime,
    ));
    let context = format!(
        "Latest user input: {}\nRecent event history:\n{}\nReturn: decision=<respond|ignore|question> reason=<short> question=<text|none>.",
        input_text, history
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
                name: "decision".to_string(),
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
        name: "decision".to_string(),
        text: response.text,
    }
}

async fn run_decision_debug(
    input_text: &str,
    submodule_outputs_raw: Option<&str>,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    append_user_provided_submodule_output_events(state, submodule_outputs_raw).await?;
    let history = if include_history {
        format_event_history(
            state,
            state.limits.decision_history,
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
    let decision_instructions = overrides
        .decision
        .clone()
        .unwrap_or_else(|| state.decision_instructions.clone());
    let adapter = ResponseApiAdapter::new(build_config(
        compose_instructions(&base_instructions, &decision_instructions),
        &state.modules.runtime,
    ));
    let context = format!(
        "Recent event history:\n{}\nReturn: decision=<respond|ignore|question> reason=<short> question=<text|none>.",
        history
    );
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
    emit_debug_module_events(
        state,
        "decision",
        "module_only",
        input_text,
        &context,
        &response,
        Some(&decision_event),
    )
    .await;
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
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let history = if include_history {
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
    let context = format!("User input: {}\nRecent events:\n{}", input_text, history);
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
    emit_debug_module_events(
        state,
        name,
        "module_only",
        input_text,
        &context,
        &response,
        Some(&response_event),
    )
    .await;
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

async fn emit_debug_module_events(
    state: &AppState,
    module: &str,
    mode: &str,
    input_text: &str,
    context: &str,
    response: &crate::llm::LlmResponse,
    history_event: Option<&Event>,
) {
    let history_event_id = history_event.map(|event| event.event_id.as_str()).unwrap_or("");
    let history_event_ts = history_event.map(|event| event.ts.as_str()).unwrap_or("");
    let worklog_event = build_event(
        "debug",
        "text",
        json!({
            "input": input_text,
            "output": response.text.clone(),
            "module": module,
            "mode": mode,
            "history_event_id": history_event_id,
            "history_event_ts": history_event_ts,
        }),
        vec![
            "debug".to_string(),
            "worklog".to_string(),
            format!("module:{}", module),
            format!("mode:{}", mode),
        ],
    );
    record_event(state, worklog_event).await;

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

async fn emit_debug_input_worklog(state: &AppState, input_text: &str, input_event: &Event) {
    let worklog_event = build_event(
        "debug",
        "text",
        json!({
            "input": input_text,
            "output": "",
            "module": "input",
            "mode": "input",
            "history_event_id": input_event.event_id,
            "history_event_ts": input_event.ts,
        }),
        vec![
            "debug".to_string(),
            "worklog".to_string(),
            "module:input".to_string(),
            "mode:input".to_string(),
        ],
    );
    record_event(state, worklog_event).await;
}

async fn append_user_provided_submodule_output_events(
    state: &AppState,
    raw: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    let outputs = parse_submodule_outputs(raw);
    for (name, text) in outputs {
        let event = build_event(
            "internal",
            "text",
            json!({ "text": text }),
            vec!["submodule".to_string(), format!("module:{}", name)],
        );
        record_event(state, event).await;
    }
    Ok(())
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
                name: name.clone(),
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
                name: name.clone(),
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
    let question = extract_field(text, "question=", &["decision=", "reason="]).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    DecisionParsed {
        decision,
        reason,
        question,
    }
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
