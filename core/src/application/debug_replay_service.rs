use axum::http::StatusCode;
use std::collections::HashSet;

use crate::app_state::AppState;
use crate::application::execution_service::{
    current_prompt_overrides, format_activation_context, format_hard_trigger_results,
    format_soft_recommendations, load_active_module_instructions, run_all_submodules_debug,
    run_decision_debug, run_submodule_debug, run_submodule_tool,
};
use crate::application::history_service::{
    format_event_lines, is_user_input_event, visible_events_before_anchor,
};
use crate::application::router_service::run_router;
use crate::debug_api::{
    DebugPromptOverridesPayload, DebugReplayTurnCompareRequest, DebugReplayTurnCompareResponse,
    DebugReplayTurnCompareVariantResponse, DebugReplayTurnRequest, DebugReplayTurnResponse,
};
use crate::event::Event;
use crate::input_ingress::RouterInput;
use crate::prompts::PromptOverrides;

const REPLAY_LOOKBACK_LIMIT: usize = 5_000;

struct ReplayTurnContext {
    input: String,
    original_output: String,
    history: String,
    excluded_event_ids: HashSet<String>,
}

pub(crate) async fn replay_turn(
    state: &AppState,
    payload: DebugReplayTurnRequest,
) -> Result<DebugReplayTurnResponse, (StatusCode, String)> {
    let module = normalize_replay_module(payload.module.as_deref());
    let context = resolve_replay_turn_context(state, payload.event_id.as_str()).await?;
    let overrides = build_replay_prompt_overrides(state, payload.prompt_overrides).await;
    let output = execute_replay_module(
        state,
        module.as_str(),
        context.input.as_str(),
        &context.excluded_event_ids,
        &overrides,
    )
    .await?;
    Ok(DebugReplayTurnResponse {
        event_id: payload.event_id,
        module,
        input: context.input,
        original_output: context.original_output,
        history: context.history,
        output,
    })
}

pub(crate) async fn compare_replay_turn(
    state: &AppState,
    payload: DebugReplayTurnCompareRequest,
) -> Result<DebugReplayTurnCompareResponse, (StatusCode, String)> {
    let module = normalize_replay_module(payload.module.as_deref());
    let context = resolve_replay_turn_context(state, payload.event_id.as_str()).await?;
    let variant_a_overrides = build_replay_prompt_overrides(state, payload.variant_a).await;
    let variant_b_overrides = build_replay_prompt_overrides(state, payload.variant_b).await;
    let variant_a_output = execute_replay_module(
        state,
        module.as_str(),
        context.input.as_str(),
        &context.excluded_event_ids,
        &variant_a_overrides,
    )
    .await?;
    let variant_b_output = execute_replay_module(
        state,
        module.as_str(),
        context.input.as_str(),
        &context.excluded_event_ids,
        &variant_b_overrides,
    )
    .await?;
    Ok(DebugReplayTurnCompareResponse {
        event_id: payload.event_id,
        module,
        input: context.input,
        original_output: context.original_output,
        history: context.history,
        variant_a: DebugReplayTurnCompareVariantResponse {
            label: "A".to_string(),
            output: variant_a_output,
        },
        variant_b: DebugReplayTurnCompareVariantResponse {
            label: "B".to_string(),
            output: variant_b_output,
        },
    })
}

async fn resolve_replay_turn_context(
    state: &AppState,
    event_id: &str,
) -> Result<ReplayTurnContext, (StatusCode, String)> {
    let target = state
        .services
        .event_store
        .get_by_id(event_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "event not found".to_string()))?;
    if target.source != "assistant" || !target.meta.tags.iter().any(|tag| tag == "response") {
        return Err((
            StatusCode::BAD_REQUEST,
            "event must be an assistant response".to_string(),
        ));
    }

    let before_events = visible_events_before_anchor(
        state,
        REPLAY_LOOKBACK_LIMIT,
        target.ts.as_str(),
        target.event_id.as_str(),
        None,
    )
    .await;
    let input = before_events
        .iter()
        .rev()
        .find(|event| is_user_input_event(event))
        .and_then(event_text)
        .map(str::to_string)
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "no user input found before target event".to_string(),
            )
        })?;

    let mut excluded_event_ids = HashSet::from([target.event_id.clone()]);
    let after_events = state
        .services
        .event_store
        .list_after_anchor(
            target.ts.as_str(),
            target.event_id.as_str(),
            REPLAY_LOOKBACK_LIMIT,
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    excluded_event_ids.extend(after_events.into_iter().map(|event| event.event_id));

    Ok(ReplayTurnContext {
        input,
        original_output: event_text(&target).unwrap_or("").to_string(),
        history: format_event_lines(&before_events),
        excluded_event_ids,
    })
}

async fn execute_replay_module(
    state: &AppState,
    module: &str,
    input: &str,
    excluded_event_ids: &HashSet<String>,
    overrides: &PromptOverrides,
) -> Result<String, (StatusCode, String)> {
    let module_instructions = load_active_module_instructions(state, overrides).await;
    if module == "router" {
        let router_input = RouterInput::from_text("message", input.to_string());
        let router_output = run_router(
            &router_input,
            &module_instructions,
            &state.runtime.modules,
            state,
            overrides,
            true,
            |module_name, activation_snapshot, instructions, focus| {
                let module_name = module_name.to_string();
                let activation_snapshot = activation_snapshot.clone();
                let instructions = instructions.to_string();
                let focus = focus.map(str::to_string);
                let input = input.to_string();
                async move {
                    run_submodule_tool(
                        state,
                        &input,
                        &activation_snapshot,
                        &module_name,
                        &instructions,
                        focus.as_deref(),
                    )
                    .await
                }
            },
        )
        .await;
        return Ok(format!(
            "<active_concepts_and_arousal>\n{}\n</active_concepts_and_arousal>\n\n<outputs_from_immediately_executed_submodules>\n{}\n</outputs_from_immediately_executed_submodules>\n\n<candidate_submodules_by_interest_match>\n{}\n</candidate_submodules_by_interest_match>\n\n<recalled_event_history>\n{}\n</recalled_event_history>",
            format_activation_context(&router_output.active_concepts_and_arousal),
            format_hard_trigger_results(&router_output.hard_trigger_results),
            format_soft_recommendations(&router_output.soft_recommendations),
            router_output.recalled_event_history,
        ));
    }
    if module == "decision" {
        return run_decision_debug(
            input,
            None,
            None,
            true,
            None,
            excluded_event_ids,
            state,
            &module_instructions,
            overrides,
        )
        .await;
    }
    if module == "submodules" {
        return run_all_submodules_debug(input, true, None, excluded_event_ids, state, overrides)
            .await;
    }
    run_submodule_debug(
        module,
        input,
        None,
        true,
        None,
        excluded_event_ids,
        state,
        overrides,
    )
    .await
}

async fn build_replay_prompt_overrides(
    state: &AppState,
    payload: Option<DebugPromptOverridesPayload>,
) -> PromptOverrides {
    let mut overrides = current_prompt_overrides(state).await;
    let Some(payload) = payload else {
        return overrides;
    };
    if let Some(base) = payload.base {
        overrides.base = Some(base);
    }
    if let Some(router) = payload.router {
        overrides.router = Some(router);
    }
    if let Some(decision) = payload.decision {
        overrides.decision = Some(decision);
    }
    if let Some(self_improvement) = payload.self_improvement {
        overrides.self_improvement = Some(self_improvement);
    }
    for item in payload.submodules {
        overrides.submodules.insert(item.name, item.instructions);
    }
    overrides
}

fn normalize_replay_module(value: Option<&str>) -> String {
    let trimmed = value.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        "decision".to_string()
    } else {
        trimmed.to_string()
    }
}

fn event_text(event: &Event) -> Option<&str> {
    event.payload.get("text").and_then(|value| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::normalize_replay_module;

    #[test]
    fn normalize_replay_module_defaults_to_decision() {
        assert_eq!(normalize_replay_module(None), "decision");
        assert_eq!(normalize_replay_module(Some("  ")), "decision");
        assert_eq!(normalize_replay_module(Some("router")), "router");
    }
}
