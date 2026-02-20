use crate::application::debug_service;
use crate::application::execution_service::{
    current_prompt_overrides, load_active_module_instructions, run_decision, run_submodule_tool,
};
use crate::application::router_service::run_router;
use crate::{AppState, DebugRunRequest, DebugRunResponse};

use axum::http::StatusCode;

pub(crate) async fn run_debug_module(
    state: &AppState,
    name: String,
    payload: DebugRunRequest,
) -> Result<DebugRunResponse, (StatusCode, String)> {
    debug_service::run_debug_module(state, name, payload).await
}

pub(crate) async fn handle_input(raw: String, state: &AppState) {
    let Ok(input_text) = debug_service::parse_and_append_input(&raw, state).await else {
        return;
    };

    let overrides = current_prompt_overrides(state).await;
    let module_instructions = load_active_module_instructions(state, &overrides).await;
    let input_text_for_router = input_text.clone();

    let router_output = run_router(
        &input_text,
        &module_instructions,
        &state.modules,
        state,
        &overrides,
        |module_name, activation_snapshot, instructions, focus| {
            let module_name = module_name.to_string();
            let activation_snapshot = activation_snapshot.clone();
            let instructions = instructions.to_string();
            let focus = focus.map(str::to_string);
            let input_text = input_text_for_router.clone();
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

    let _decision_output = run_decision(
        &input_text,
        &router_output,
        &state.modules,
        state,
        &module_instructions,
        &overrides,
    )
    .await;
}
