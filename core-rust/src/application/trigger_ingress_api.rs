use axum::http::StatusCode;
use serde_json::Value;

use crate::app_state::AppState;
use crate::application::event_service::record_event;
use crate::debug_api::{DebugTriggerRequest, DebugTriggerResponse};
use crate::event::contracts::named_trigger;

pub(crate) async fn trigger_improvement(
    state: &AppState,
    payload: DebugTriggerRequest,
) -> Result<DebugTriggerResponse, (StatusCode, String)> {
    let event = payload.event.trim();
    if event.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "event is required".to_string()));
    }

    let trigger_event = named_trigger(
        "system",
        event,
        payload
            .payload
            .unwrap_or_else(|| Value::Object(Default::default())),
    );
    let trigger_event_id = trigger_event.event_id.clone();
    record_event(state, trigger_event).await;

    Ok(DebugTriggerResponse {
        event_id: trigger_event_id,
    })
}
