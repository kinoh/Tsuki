use axum::http::StatusCode;
use serde_json::Value;

use crate::event::build_event;
use crate::{AppState, DebugTriggerRequest, DebugTriggerResponse};

pub(crate) async fn trigger_improvement(
    state: &AppState,
    payload: DebugTriggerRequest,
) -> Result<DebugTriggerResponse, (StatusCode, String)> {
    let event = payload.event.trim();
    if event.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "event is required".to_string()));
    }

    let trigger_event = build_event(
        "system",
        "text",
        payload
            .payload
            .unwrap_or_else(|| Value::Object(Default::default())),
        vec![event.to_string()],
    );
    let trigger_event_id = trigger_event.event_id.clone();
    crate::record_event(state, trigger_event).await;

    Ok(DebugTriggerResponse {
        event_id: trigger_event_id,
    })
}
