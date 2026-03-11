use axum::http::StatusCode;
use futures::future::join_all;
use serde::Deserialize;
use serde_json::json;
use std::time::Instant;
use std::{collections::HashMap, sync::Arc};
use tokio::runtime::Handle;

use crate::app_state::AppState;
use crate::application::event_service::record_event;
use crate::application::history_service::{format_decision_debug_history, format_event_history};
use crate::application::module_bootstrap::{ModuleRuntime, Modules};
use crate::application::router_service::{
    activation_snapshot_from_router_output, ActivationSnapshot, HardTriggerResult, RouterOutput,
};
use crate::activation_concept_graph::VisibleSkill;
use crate::application::usage_service::DbLlmUsageRecorder;
use crate::event::contracts::{decision_text, llm_error, llm_raw, role_text_output};
use crate::llm::{
    build_response_api_llm, LlmAdapter, LlmRequest, LlmUsageContext, LlmUsageRecorder,
    ResponseApiConfig, ToolError, ToolHandler,
};
use crate::module_registry::ModuleRegistryReader;
use crate::prompts::PromptOverrides;
use crate::tools::EMIT_USER_REPLY_TOOL;

const SUBMODULE_TOOL_PREFIX: &str = "run_submodule__";

#[derive(Debug, Clone)]
struct ModuleOutput {
    text: String,
}

#[derive(Debug, Deserialize)]
struct SubmoduleToolArgs {
    #[serde(default)]
    execution_reason: Option<String>,
}

struct DecisionParsed {
    decision: String,
    reason: Option<String>,
}

struct DecisionContextTemplateVars<'a> {
    latest_user_input: &'a str,
    active_concepts_and_arousal: &'a str,
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

pub(crate) async fn run_decision(
    input_text: &str,
    router_output: &RouterOutput,
    modules: &Modules,
    state: &AppState,
    module_instructions: &HashMap<String, String>,
    overrides: &PromptOverrides,
) -> String {
    let decision_started = Instant::now();
    println!(
        "PERF decision stage=start input_len={} hard_trigger_results={} soft_recommendations={}",
        input_text.len(),
        router_output.hard_trigger_results.len(),
        router_output.soft_recommendations.len()
    );
    let history_started = Instant::now();
    let history =
        format_event_history(state, state.config.limits.decision_history, None, None).await;
    println!(
        "PERF decision stage=history ms={} history_len={}",
        history_started.elapsed().as_millis(),
        history.len()
    );
    let base_instructions = state.prompts.base_or_default(&overrides);
    let decision_instructions = state.prompts.decision_or_default(&overrides);
    let activation_snapshot = activation_snapshot_from_router_output(router_output);
    let handler = DecisionToolHandler {
        state: state.clone(),
        input_text: input_text.to_string(),
        activation_snapshot: activation_snapshot.clone(),
        base_handler: modules.runtime.tool_handler.clone(),
        module_instructions: module_instructions.clone(),
    };
    let usage_recorder: Arc<dyn LlmUsageRecorder> =
        Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
    let visible_mcp_tools = state
        .services
        .mcp_registry
        .available_tools()
        .into_iter()
        .filter(|tool| {
            tool_name(tool)
                .map(|name| {
                    router_output
                        .mcp_visible_tools
                        .iter()
                        .any(|item| item == name)
                })
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let adapter = build_response_api_llm(build_config_with_tools_and_handler(
        compose_decision_instructions(
            &base_instructions,
            &decision_instructions,
            &visible_mcp_tools,
            !router_output.visible_skills.is_empty(),
        ),
        &modules.runtime,
        decision_tools(
            &modules.runtime.tools,
            visible_mcp_tools.clone(),
            module_instructions.keys().cloned(),
        ),
        Arc::new(handler),
        Some(LlmUsageContext::new("user", "decision")),
        Some(usage_recorder),
    ));
    let activation_concepts =
        format_activation_context(&activation_snapshot.active_concepts_and_arousal);
    let executed_submodule_outputs =
        format_hard_trigger_results(&router_output.hard_trigger_results);
    let submodule_candidates =
        format_soft_recommendations(&activation_snapshot.soft_recommendations);
    let context = render_decision_context_template(
        &state.config.input.decision_context_template,
        DecisionContextTemplateVars {
            latest_user_input: input_text,
            active_concepts_and_arousal: &activation_concepts,
            outputs_from_immediately_executed_submodules: &executed_submodule_outputs,
            candidate_submodules_by_interest_match: &submodule_candidates,
            recent_event_history: &history,
        },
    );
    let context = append_visible_mcp_tool_contracts(context, &visible_mcp_tools);
    let context = append_visible_skill_summaries(context, &router_output.visible_skills);

    let llm_started = Instant::now();
    let response = match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => {
            println!(
                "PERF decision stage=respond ms={} ok=true output_len={} tool_calls={}",
                llm_started.elapsed().as_millis(),
                response.text.len(),
                response.tool_calls.len()
            );
            response
        }
        Err(err) => {
            let error_detail = err.to_string();
            println!(
                "PERF decision stage=respond ms={} ok=false error={}",
                llm_started.elapsed().as_millis(),
                error_detail
            );
            let error_text = format!("error: {}", error_detail);
            let error_event = decision_text(error_text, true);
            record_event(state, error_event).await;
            emit_debug_module_error_event(state, "decision", "runtime", &context, &error_detail)
                .await;
            println!(
                "PERF decision stage=end total_ms={} decision=error",
                decision_started.elapsed().as_millis()
            );
            return format!("error: {}", error_detail);
        }
    };

    let parsed = parse_decision(&response.text);
    let response =
        if parsed.decision == "respond" && !has_tool_call(&response, EMIT_USER_REPLY_TOOL) {
            repair_missing_emit_user_reply(
                modules,
                state,
                &base_instructions,
                &decision_instructions,
                &context,
                &response,
            )
            .await
            .unwrap_or(response)
        } else {
            response
        };
    let parsed = parse_decision(&response.text);
    let reason_text = parsed.reason.unwrap_or_else(|| "none".to_string());
    let decision_event = decision_text(
        format!("decision={} reason={}", parsed.decision, reason_text),
        false,
    );
    record_event(state, decision_event).await;
    emit_debug_module_events(state, "decision", "runtime", &context, &response).await;
    println!(
        "PERF decision stage=end total_ms={} decision={}",
        decision_started.elapsed().as_millis(),
        parsed.decision
    );

    response.text
}

pub(crate) async fn run_decision_debug(
    input_text: &str,
    context_override: Option<&str>,
    submodule_outputs_raw: Option<&str>,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &std::collections::HashSet<String>,
    state: &AppState,
    router_output: &RouterOutput,
    module_instructions: &HashMap<String, String>,
    overrides: &PromptOverrides,
) -> Result<String, (StatusCode, String)> {
    let history = if context_override.is_some() {
        String::new()
    } else if include_history {
        format_decision_debug_history(
            state,
            state.config.limits.decision_history,
            history_cutoff_ts,
            Some(excluded_event_ids),
            submodule_outputs_raw,
        )
        .await
    } else {
        "none".to_string()
    };
    let base_instructions = state.prompts.base_or_default(&overrides);
    let decision_instructions = state.prompts.decision_or_default(&overrides);
    let activation_snapshot = activation_snapshot_from_router_output(router_output);
    let handler = DecisionToolHandler {
        state: state.clone(),
        input_text: input_text.to_string(),
        activation_snapshot: activation_snapshot.clone(),
        base_handler: state.runtime.modules.runtime.tool_handler.clone(),
        module_instructions: module_instructions.clone(),
    };
    let usage_recorder: Arc<dyn LlmUsageRecorder> =
        Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
    let visible_mcp_tools = state
        .services
        .mcp_registry
        .available_tools()
        .into_iter()
        .filter(|tool| {
            tool_name(tool)
                .map(|name| {
                    router_output
                        .mcp_visible_tools
                        .iter()
                        .any(|item| item == name)
                })
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let adapter = build_response_api_llm(build_config_with_tools_and_handler(
        compose_decision_instructions(
            &base_instructions,
            &decision_instructions,
            &visible_mcp_tools,
            !router_output.visible_skills.is_empty(),
        ),
        &state.runtime.modules.runtime,
        decision_tools(
            &state.runtime.modules.runtime.tools,
            visible_mcp_tools.clone(),
            module_instructions.keys().cloned(),
        ),
        Arc::new(handler),
        Some(LlmUsageContext::new("user", "decision")),
        Some(usage_recorder),
    ));
    let context = context_override.map(str::to_string).unwrap_or_else(|| {
        let activation_concepts =
            format_activation_context(&activation_snapshot.active_concepts_and_arousal);
        let executed_submodule_outputs =
            format_hard_trigger_results(&router_output.hard_trigger_results);
        let submodule_candidates =
            format_soft_recommendations(&activation_snapshot.soft_recommendations);
        render_decision_context_template(
            &state.config.input.decision_context_template,
            DecisionContextTemplateVars {
                latest_user_input: input_text,
                active_concepts_and_arousal: &activation_concepts,
                outputs_from_immediately_executed_submodules: &executed_submodule_outputs,
                candidate_submodules_by_interest_match: &submodule_candidates,
                recent_event_history: &history,
            },
        )
    });
    let context = append_visible_mcp_tool_contracts(context, &visible_mcp_tools);
    let context = append_visible_skill_summaries(context, &router_output.visible_skills);
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
    let decision_event = decision_text(
        format!("decision={} reason={}", parsed.decision, reason_text),
        false,
    );
    record_event(state, decision_event).await;
    emit_debug_module_events(state, "decision", "module_only", &context, &response).await;
    Ok(response.text)
}

pub(crate) async fn run_all_submodules_debug(
    input_text: &str,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &std::collections::HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let module_names = state
        .runtime
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

pub(crate) async fn run_submodule_debug(
    name: &str,
    input_text: &str,
    context_override: Option<&str>,
    include_history: bool,
    history_cutoff_ts: Option<&str>,
    excluded_event_ids: &std::collections::HashSet<String>,
    state: &AppState,
) -> Result<String, (StatusCode, String)> {
    let history = if context_override.is_some() {
        String::new()
    } else if include_history {
        format_event_history(
            state,
            state.config.limits.submodule_history,
            history_cutoff_ts,
            Some(excluded_event_ids),
        )
        .await
    } else {
        "none".to_string()
    };
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = state.prompts.base_or_default(&overrides);
    let module_defs = state
        .runtime
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
    let usage_recorder: Arc<dyn LlmUsageRecorder> =
        Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
    let adapter = build_response_api_llm(build_config(
        compose_instructions(&base_instructions, &instructions),
        &state.runtime.modules.runtime,
        Some(LlmUsageContext::new("user", format!("submodule:{}", name))),
        Some(usage_recorder),
    ));
    let context = context_override.map(str::to_string).unwrap_or_else(|| {
        render_submodule_context_template(
            &state.config.input.submodule_context_template,
            input_text,
            "none",
            "none",
            &history,
            "none",
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
            emit_debug_module_error_event(state, name, "module_only", &context, &error).await;
            return Err((StatusCode::INTERNAL_SERVER_ERROR, error));
        }
    };
    let response_event = role_text_output(
        format!("submodule:{}", name).as_str(),
        "submodule",
        response.text.clone(),
        false,
    );
    record_event(state, response_event).await;
    emit_debug_module_events(state, name, "module_only", &context, &response).await;
    Ok(response.text)
}

pub(crate) async fn run_submodule_tool(
    state: &AppState,
    input_text: &str,
    activation_snapshot: &ActivationSnapshot,
    module_name: &str,
    module_instructions: &str,
    execution_reason: Option<&str>,
) -> Result<String, ToolError> {
    let history =
        format_event_history(state, state.config.limits.submodule_history, None, None).await;
    let overrides = current_prompt_overrides(state).await;
    let base_instructions = state.prompts.base_or_default(&overrides);
    let instructions = compose_instructions(&base_instructions, module_instructions);
    let usage_recorder: Arc<dyn LlmUsageRecorder> =
        Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
    let adapter = build_response_api_llm(build_config(
        instructions,
        &state.runtime.modules.runtime,
        Some(LlmUsageContext::new(
            "user",
            format!("submodule:{}", module_name),
        )),
        Some(usage_recorder),
    ));
    let context = render_submodule_context_template(
        &state.config.input.submodule_context_template,
        input_text,
        &format_activation_context(&activation_snapshot.active_concepts_and_arousal),
        &format_soft_recommendations(&activation_snapshot.soft_recommendations),
        &history,
        execution_reason.unwrap_or("none"),
    );
    let output = run_module(
        state,
        module_name.to_string(),
        "submodule",
        adapter,
        context,
    )
    .await;
    Ok(output.text)
}

pub(crate) async fn current_prompt_overrides(state: &AppState) -> PromptOverrides {
    state.prompts.overrides.read().await.clone()
}

pub(crate) async fn load_active_module_instructions(
    state: &AppState,
    overrides: &PromptOverrides,
) -> HashMap<String, String> {
    let module_defs = match state.runtime.modules.registry.list_active().await {
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
    module_instructions
}

fn parse_decision(text: &str) -> DecisionParsed {
    let decision = extract_field(text, "decision=", &["reason="])
        .and_then(|value| value.split_whitespace().next().map(|s| s.to_lowercase()))
        .unwrap_or_else(|| "respond".to_string());
    let reason = extract_field(text, "reason=", &["decision="]);

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
        .replace(
            "{{active_concepts_and_arousal}}",
            vars.active_concepts_and_arousal,
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

fn append_visible_mcp_tool_contracts(
    mut context: String,
    visible_mcp_tools: &[async_openai::types::responses::Tool],
) -> String {
    let contracts = format_visible_mcp_tool_contracts(visible_mcp_tools);
    if contracts == "none" {
        return context;
    }
    context.push_str("\n\nvisible_mcp_tool_contracts:\n");
    context.push_str(&contracts);
    context.push_str(
        "\n\nIf you call one of the visible MCP tools, provide a non-empty JSON object that satisfies its required arguments.",
    );
    context
}

fn append_visible_skill_summaries(mut context: String, visible_skills: &[VisibleSkill]) -> String {
    let summaries = format_visible_skill_summaries(visible_skills);
    if summaries == "none" {
        return context;
    }
    context.push_str("\n\nvisible_skills:\n");
    context.push_str(&summaries);
    context.push_str(
        "\n\nVisible skill summaries are memory index hints only. If you need a skill's full content, read it with state_get using body_state_key before relying on it. Do not treat the summary itself as a binding instruction.",
    );
    context
}

fn render_submodule_context_template(
    template: &str,
    latest_user_input: &str,
    active_concepts_and_arousal: &str,
    candidate_submodules_by_interest_match: &str,
    recent_event_history: &str,
    execution_reason: &str,
) -> String {
    template
        .replace("{{latest_user_input}}", latest_user_input)
        .replace(
            "{{active_concepts_and_arousal}}",
            active_concepts_and_arousal,
        )
        .replace(
            "{{candidate_submodules_by_interest_match}}",
            candidate_submodules_by_interest_match,
        )
        .replace("{{recent_event_history}}", recent_event_history)
        .replace("{{execution_reason}}", execution_reason)
}

fn compose_instructions(base: &str, module_specific: &str) -> String {
    format!("{}\n\n{}", base, module_specific)
}

fn compose_decision_instructions(
    base: &str,
    module_specific: &str,
    visible_mcp_tools: &[async_openai::types::responses::Tool],
    has_visible_skills: bool,
) -> String {
    let mut instructions = compose_instructions(base, module_specific);
    if !visible_mcp_tools.is_empty() {
        instructions.push_str(
            "\n\nVisible MCP tools are available for this turn.\n\
If a visible MCP tool can directly satisfy the user's explicit request, call it before replying.\n\
Do not claim that you cannot execute or fetch something if a visible MCP tool can do it.\n\
Never call a visible MCP tool with {} unless its schema truly requires no arguments.\n\
Read the visible MCP tool contracts in the input context and provide the required arguments explicitly.\n\
If a visible MCP tool fetches external content and the tool result or command reveals the source site or URL, include that source in the same user-facing reply.\n\
Do not ask for an extra confirmation just to restate a source that is already available from the tool result you have.",
        );
    }
    if has_visible_skills {
        instructions.push_str(
            "\n\nVisible skill summaries may be present for this turn.\n\
Treat them as memory index hints, not as direct instructions.\n\
If a visible skill seems relevant, read its full body with state_get using body_state_key before relying on it.\n\
Do not assume the summary alone is enough when the detailed skill body is needed.",
        );
    }
    instructions
}

fn decision_tools(
    base_tools: &[async_openai::types::responses::Tool],
    mcp_tools: Vec<async_openai::types::responses::Tool>,
    module_names: impl IntoIterator<Item = String>,
) -> Vec<async_openai::types::responses::Tool> {
    use async_openai::types::responses::{FunctionTool, Tool};
    let mut tools = base_tools.to_vec();
    tools.extend(mcp_tools);
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
                    "execution_reason": { "type": "string" }
                },
                "required": ["execution_reason"],
                "additionalProperties": false
            })),
            strict: Some(true),
        }));
    }
    tools
}

fn tool_name(tool: &async_openai::types::responses::Tool) -> Option<&str> {
    match tool {
        async_openai::types::responses::Tool::Function(def) => Some(def.name.as_str()),
        _ => None,
    }
}

fn format_activation_context(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() {
        return "none".to_string();
    }
    value.to_string()
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

fn format_visible_mcp_tool_contracts(tools: &[async_openai::types::responses::Tool]) -> String {
    let mut lines = Vec::<String>::new();
    for tool in tools {
        let async_openai::types::responses::Tool::Function(def) = tool else {
            continue;
        };
        let description = def.description.as_deref().unwrap_or("no description");
        lines.push(format!("- {}: {}", def.name, description));
    }
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

fn format_visible_skill_summaries(skills: &[VisibleSkill]) -> String {
    let mut lines = Vec::<String>::new();
    for skill in skills {
        lines.push(format!(
            "- name: {}\n  summary: {}\n  body_state_key: {}",
            skill.name, skill.summary, skill.body_state_key
        ));
    }
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "…"
}

async fn emit_debug_module_events(
    state: &AppState,
    module: &str,
    mode: &str,
    context: &str,
    response: &crate::llm::LlmResponse,
) {
    let raw_event = llm_raw(
        module_owner_source(module).as_str(),
        json!({
            "raw": response.raw.clone(),
            "context": context,
            "output_text": response.text.clone(),
            "tool_calls": response.tool_calls.clone(),
            "mode": mode,
        }),
        vec![format!("mode:{}", mode)],
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
    let error_event = llm_error(
        module_owner_source(module).as_str(),
        json!({
            "mode": mode,
            "context": context,
            "error": error,
        }),
        vec![format!("mode:{}", mode)],
    );
    record_event(state, error_event).await;
}

fn module_owner_source(module: &str) -> String {
    if module.eq_ignore_ascii_case("router") {
        return "router".to_string();
    }
    if module.eq_ignore_ascii_case("decision") {
        return "decision".to_string();
    }
    format!("submodule:{}", module)
}

async fn run_module(
    state: &AppState,
    name: String,
    role_tag: &'static str,
    adapter: Arc<dyn LlmAdapter>,
    input: String,
) -> ModuleOutput {
    let context = input;
    match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => {
            let response_event = role_text_output(
                owner_source_for_role(role_tag, &name).as_str(),
                role_tag,
                response.text.clone(),
                false,
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
            let error_event = role_text_output(
                owner_source_for_role(role_tag, &name).as_str(),
                role_tag,
                error_text,
                true,
            );
            record_event(state, error_event).await;
            emit_debug_module_error_event(state, &name, "runtime", &context, &error_detail).await;
            ModuleOutput {
                text: format!("error: {}", error_detail),
            }
        }
    }
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
                    args.execution_reason.as_deref(),
                ))
            })?;
            Ok(output)
        } else {
            self.base_handler.handle(tool_name, arguments)
        }
    }
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

fn build_config(
    instructions: String,
    runtime: &ModuleRuntime,
    usage_context: Option<LlmUsageContext>,
    usage_recorder: Option<Arc<dyn LlmUsageRecorder>>,
) -> ResponseApiConfig {
    ResponseApiConfig {
        model: runtime.model.clone(),
        instructions,
        temperature: runtime.temperature,
        max_output_tokens: runtime.max_output_tokens,
        tools: runtime.tools.clone(),
        tool_handler: Some(runtime.tool_handler.clone()),
        usage_recorder,
        usage_context,
        max_tool_rounds: runtime.max_tool_rounds,
    }
}

fn build_config_with_tools_and_handler(
    instructions: String,
    runtime: &ModuleRuntime,
    tools: Vec<async_openai::types::responses::Tool>,
    tool_handler: Arc<dyn ToolHandler>,
    usage_context: Option<LlmUsageContext>,
    usage_recorder: Option<Arc<dyn LlmUsageRecorder>>,
) -> ResponseApiConfig {
    ResponseApiConfig {
        model: runtime.model.clone(),
        instructions,
        temperature: runtime.temperature,
        max_output_tokens: runtime.max_output_tokens,
        tools,
        tool_handler: Some(tool_handler),
        usage_recorder,
        usage_context,
        max_tool_rounds: runtime.max_tool_rounds,
    }
}

fn has_tool_call(response: &crate::llm::LlmResponse, tool_name: &str) -> bool {
    response
        .tool_calls
        .iter()
        .any(|call| call.name == tool_name)
}

async fn repair_missing_emit_user_reply(
    modules: &Modules,
    state: &AppState,
    base_instructions: &str,
    decision_instructions: &str,
    original_context: &str,
    response: &crate::llm::LlmResponse,
) -> Option<crate::llm::LlmResponse> {
    let emit_tool = modules
        .runtime
        .tools
        .iter()
        .find(|tool| tool_name(tool) == Some(EMIT_USER_REPLY_TOOL))
        .cloned()?;
    let repair_instructions = format!(
        "{}\n\n{}\n\nDecision contract repair:\nYou already decided to respond but failed to call emit_user_reply.\nYou must call emit_user_reply exactly once using the available tool.\nDo not call any other tool.\nAfter the tool call, output exactly one line: decision=respond reason=contract_repair.",
        base_instructions, decision_instructions
    );
    let tool_results = response
        .tool_calls
        .iter()
        .map(|call| {
            format!(
                "- {}: output={} error={}",
                call.name,
                truncate(&call.output, 400),
                call.error.as_deref().unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let repair_context = format!(
        "{original_context}\n\ntool_call_results_from_previous_attempt:\n{tool_results}\n\nRepair requirement:\nCall emit_user_reply now using the gathered tool results."
    );
    let usage_recorder: Arc<dyn LlmUsageRecorder> =
        Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
    let adapter = build_response_api_llm(ResponseApiConfig {
        model: modules.runtime.model.clone(),
        instructions: repair_instructions,
        temperature: modules.runtime.temperature,
        max_output_tokens: modules.runtime.max_output_tokens,
        tools: vec![emit_tool],
        tool_handler: Some(modules.runtime.tool_handler.clone()),
        usage_recorder: Some(usage_recorder),
        usage_context: Some(LlmUsageContext::new("user", "decision-repair")),
        max_tool_rounds: 1,
    });

    match adapter
        .respond(LlmRequest {
            input: repair_context,
        })
        .await
    {
        Ok(repaired) if has_tool_call(&repaired, EMIT_USER_REPLY_TOOL) => Some(repaired),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_visible_skill_summaries_renders_expected_shape() {
        let skills = vec![VisibleSkill {
            name: "skill:gentle_conversation_guidance".to_string(),
            summary: "Low-pressure supportive conversational guidance".to_string(),
            body_state_key: "skill:gentle_conversation_guidance".to_string(),
            score: 0.83,
        }];
        let rendered = format_visible_skill_summaries(&skills);
        assert!(rendered.contains("name: skill:gentle_conversation_guidance"));
        assert!(rendered.contains("summary: Low-pressure supportive conversational guidance"));
        assert!(rendered.contains("body_state_key: skill:gentle_conversation_guidance"));
    }
}
