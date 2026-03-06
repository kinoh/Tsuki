use serde_json::{json, Value};
use tokio::sync::broadcast::error::RecvError;

use crate::{application::pipeline_service, AppState};

const SCHEDULER_NOTICE_TAG: &str = "scheduler.notice";

pub(crate) fn start_notice_consumer(state: AppState) {
    tokio::spawn(async move {
        let mut rx = state.services.tx.subscribe();
        loop {
            let event = match rx.recv().await {
                Ok(value) => value,
                Err(RecvError::Lagged(skipped)) => {
                    println!("SCHEDULER_NOTICE_CONSUMER_LAGGED skipped={}", skipped);
                    continue;
                }
                Err(RecvError::Closed) => break,
            };
            if !event
                .meta
                .tags
                .iter()
                .any(|tag| tag == SCHEDULER_NOTICE_TAG)
            {
                continue;
            }

            let input_text = scheduler_notice_input_text(&event.payload);
            let raw = json!({
                "type": "scheduler_notice",
                "text": input_text,
            })
            .to_string();
            pipeline_service::handle_input(raw, &state).await;
        }
    });
}

fn scheduler_notice_input_text(payload: &Value) -> String {
    let schedule_id = payload
        .get("schedule_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let scheduled_at = payload
        .get("scheduled_at")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let action_kind = payload
        .get("action")
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let details = payload
        .get("action")
        .map(|value| value.to_string())
        .unwrap_or_else(|| "{}".to_string());

    format!(
        "scheduler notice: schedule_id={} scheduled_at={} action.kind={} action={}",
        schedule_id, scheduled_at, action_kind, details
    )
}
