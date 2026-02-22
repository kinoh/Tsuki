use axum::http::StatusCode;
use serde_json::json;

use crate::clock::now_iso8601;
use crate::event::build_event;
use crate::{AppState, DebugImproveResponse, DebugImproveTriggerRequest};

pub(crate) async fn trigger_improvement(
    state: &AppState,
    payload: DebugImproveTriggerRequest,
) -> Result<DebugImproveResponse, (StatusCode, String)> {
    let target = payload
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("all")
        .to_string();
    let reason = payload
        .reason
        .unwrap_or_else(|| "manual trigger".to_string());

    let trigger_event = build_event(
        "system",
        "text",
        json!({
            "target": target,
            "reason": reason,
            "created_at": now_iso8601(),
        }),
        vec!["self_improvement.triggered".to_string()],
    );
    crate::record_event(state, trigger_event).await;

    Ok(DebugImproveResponse {
        proposal_id: None,
        review_event_id: None,
        apply_event_id: None,
        applied: false,
    })
}
