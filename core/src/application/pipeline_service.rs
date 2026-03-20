use crate::app_state::AppState;
use crate::application::debug_service;
use crate::application::execution_service::{
    current_prompt_overrides, load_active_module_instructions, run_decision, run_submodule_tool,
};
use crate::application::router_service::run_router;
use crate::debug_api::{DebugRunRequest, DebugRunResponse};

use axum::http::StatusCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) async fn run_debug_module(
    state: &AppState,
    name: String,
    payload: DebugRunRequest,
) -> Result<DebugRunResponse, (StatusCode, String)> {
    debug_service::run_debug_module(state, name, payload).await
}

pub(crate) async fn handle_input(raw: String, state: &AppState) {
    let trace_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let pipeline_started = Instant::now();
    println!(
        "PERF pipeline trace={} stage=start raw_len={}",
        trace_id,
        raw.len()
    );

    let parse_started = Instant::now();
    let Ok(input) = debug_service::parse_and_append_input(&raw, state).await else {
        println!(
            "PERF pipeline trace={} stage=parse_input ok=false ms={}",
            trace_id,
            parse_started.elapsed().as_millis()
        );
        return;
    };
    let input_text = input.display_text();
    println!(
        "PERF pipeline trace={} stage=parse_input ok=true ms={} input_len={}",
        trace_id,
        parse_started.elapsed().as_millis(),
        input_text.len()
    );

    let prep_started = Instant::now();
    let overrides = current_prompt_overrides(state).await;
    let module_instructions = load_active_module_instructions(state, &overrides).await;
    println!(
        "PERF pipeline trace={} stage=prepare ms={} active_modules={}",
        trace_id,
        prep_started.elapsed().as_millis(),
        module_instructions.len()
    );
    let input_for_router = input.clone();

    let router_started = Instant::now();
    let router_output = run_router(
        &input_for_router,
        &module_instructions,
        &state.runtime.modules,
        state,
        &overrides,
        false,
        |module_name, activation_snapshot, instructions, focus| {
            let module_name = module_name.to_string();
            let activation_snapshot = activation_snapshot.clone();
            let instructions = instructions.to_string();
            let focus = focus.map(str::to_string);
            let input_text = input_text.clone();
            async move {
                run_submodule_tool(
                    state,
                    &input_text,
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
    println!(
        "PERF pipeline trace={} stage=router ms={} hard_triggers={} hard_results={} soft_recommendations={}",
        trace_id,
        router_started.elapsed().as_millis(),
        router_output.hard_triggers.len(),
        router_output.hard_trigger_results.len(),
        router_output.soft_recommendations.len()
    );

    let decision_started = Instant::now();
    let _decision_output = run_decision(
        &input_text,
        router_output,
        &state.runtime.modules,
        state,
        &module_instructions,
        &overrides,
    )
    .await;
    println!(
        "PERF pipeline trace={} stage=decision ms={}",
        trace_id,
        decision_started.elapsed().as_millis(),
    );
    println!(
        "PERF pipeline trace={} stage=end total_ms={}",
        trace_id,
        pipeline_started.elapsed().as_millis(),
    );
}
