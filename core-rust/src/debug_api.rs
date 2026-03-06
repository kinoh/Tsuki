use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct DebugRunRequest {
    pub(crate) input: String,
    #[serde(default)]
    pub(crate) context_override: Option<String>,
    #[serde(default)]
    pub(crate) submodule_outputs: Option<String>,
    #[serde(default)]
    pub(crate) include_history: Option<bool>,
    #[serde(default)]
    pub(crate) history_cutoff_ts: Option<String>,
    #[serde(default)]
    pub(crate) exclude_event_ids: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) append_input_mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugRunResponse {
    pub(crate) output: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugTriggerRequest {
    pub(crate) event: String,
    #[serde(default)]
    pub(crate) payload: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugTriggerResponse {
    pub(crate) event_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugImproveProposalRequest {
    pub(crate) target: String,
    pub(crate) job_id: String,
    pub(crate) diff_text: String,
    #[serde(default)]
    pub(crate) requires_approval: Option<bool>,
    #[serde(default)]
    pub(crate) created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugImproveReviewRequest {
    pub(crate) proposal_id: String,
    pub(crate) job_id: String,
    pub(crate) target: String,
    pub(crate) decision: String,
    #[serde(default)]
    pub(crate) reviewed_by: Option<String>,
    #[serde(default)]
    pub(crate) review_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugImproveResponse {
    pub(crate) proposal_id: Option<String>,
    pub(crate) review_event_id: Option<String>,
    pub(crate) apply_event_id: Option<String>,
    pub(crate) applied: bool,
}
