use crate::app_state::AppState;
use crate::application::debug_service;
use crate::application::history_service::latest_events;
use crate::application::thought_process_service::run_basic_thought_process;

use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
    let overrides = state.prompts.overrides.read().await.clone();
    println!(
        "PERF pipeline trace={} stage=prepare ms={}",
        trace_id,
        prep_started.elapsed().as_millis()
    );

    let event_select_started = Instant::now();
    let events = latest_events(state, state.config.limits.decision_history, None, None).await;
    println!(
        "PERF pipeline trace={} stage=select_events ms={} events={}",
        trace_id,
        event_select_started.elapsed().as_millis(),
        events.len()
    );

    let thought_started = Instant::now();
    let result = run_basic_thought_process(
        state,
        events,
        &state.runtime.modules.runtime,
        &state.prompts.base_or_default(&overrides),
        &state.prompts.decision_or_default(&overrides),
    )
    .await;
    match result {
        Ok(result) => {
            println!(
                "PERF pipeline trace={} stage=thought_process ms={} actions={} action_results={}",
                trace_id,
                thought_started.elapsed().as_millis(),
                result.decision_output.actions.len(),
                result.action_results.len()
            );
        }
        Err(err) => {
            println!(
                "PERF pipeline trace={} stage=thought_process ms={} ok=false error={}",
                trace_id,
                thought_started.elapsed().as_millis(),
                err
            );
        }
    }
    println!(
        "PERF pipeline trace={} stage=end total_ms={}",
        trace_id,
        pipeline_started.elapsed().as_millis(),
    );
}
