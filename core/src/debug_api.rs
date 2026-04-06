use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub(crate) struct DebugPromptOverridesPayload {
    #[serde(default)]
    pub(crate) base: Option<String>,
    #[serde(default)]
    pub(crate) router: Option<String>,
    #[serde(default)]
    pub(crate) decision: Option<String>,
    #[serde(default)]
    pub(crate) self_improvement: Option<String>,
    #[serde(default)]
    pub(crate) submodules: Vec<DebugPromptModulePayload>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct DebugPromptModulePayload {
    pub(crate) name: String,
    pub(crate) instructions: String,
}

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
    #[serde(default)]
    pub(crate) dry_run: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugRunResponse {
    pub(crate) output: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugReplayTurnRequest {
    pub(crate) event_id: String,
    #[serde(default)]
    pub(crate) module: Option<String>,
    #[serde(default)]
    pub(crate) prompt_overrides: Option<DebugPromptOverridesPayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugReplayTurnResponse {
    pub(crate) event_id: String,
    pub(crate) module: String,
    pub(crate) input: String,
    pub(crate) original_output: String,
    pub(crate) history: String,
    pub(crate) output: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DebugReplayTurnCompareRequest {
    pub(crate) event_id: String,
    #[serde(default)]
    pub(crate) module: Option<String>,
    #[serde(default)]
    pub(crate) variant_a: Option<DebugPromptOverridesPayload>,
    #[serde(default)]
    pub(crate) variant_b: Option<DebugPromptOverridesPayload>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugReplayTurnCompareVariantResponse {
    pub(crate) label: String,
    pub(crate) output: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DebugReplayTurnCompareResponse {
    pub(crate) event_id: String,
    pub(crate) module: String,
    pub(crate) input: String,
    pub(crate) original_output: String,
    pub(crate) history: String,
    pub(crate) variant_a: DebugReplayTurnCompareVariantResponse,
    pub(crate) variant_b: DebugReplayTurnCompareVariantResponse,
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
