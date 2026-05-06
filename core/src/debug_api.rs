use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    application::thought_process_service::{
        Action, ActionResult, AvailableAction, ComponentTrace, DecisionContext, DecisionOutput,
        DeliberationContributions, ThoughtProcessRunMode, ThoughtProcessTrace,
    },
    event::Event,
};

#[derive(Debug, Deserialize)]
pub(crate) struct ThoughtProcessRunRequest {
    #[serde(default)]
    pub(crate) input: Option<String>,
    #[serde(default)]
    pub(crate) history_limit: Option<usize>,
    #[serde(default)]
    pub(crate) include_history: Option<bool>,
    #[serde(default)]
    pub(crate) history_cutoff_ts: Option<String>,
    #[serde(default)]
    pub(crate) exclude_event_ids: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) mode: Option<ThoughtProcessRunMode>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ThoughtProcessRunResponse {
    pub(crate) mode: ThoughtProcessRunMode,
    pub(crate) event_history: Vec<Event>,
    pub(crate) result: ThoughtProcessInspection,
}

#[derive(Debug, Serialize)]
pub(crate) struct ThoughtProcessEventHistoryResponse {
    pub(crate) event_history: Vec<Event>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ThoughtProcessInspection {
    pub(crate) decision_context: DecisionContext,
    pub(crate) deliberation_contributions: DeliberationContributions,
    pub(crate) decision_output: DecisionOutput,
    pub(crate) action_results: Vec<ActionResult>,
    pub(crate) trace: ThoughtProcessTrace,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThoughtProcessComponentRunRequest {
    #[serde(default)]
    pub(crate) mode: Option<ThoughtProcessRunMode>,
    #[serde(default)]
    pub(crate) event_history: Option<Vec<Event>>,
    #[serde(default)]
    pub(crate) decision_context: Option<DecisionContext>,
    #[serde(default)]
    pub(crate) deliberation_contributions: Option<DeliberationContributions>,
    #[serde(default)]
    pub(crate) available_actions: Option<Vec<AvailableAction>>,
    #[serde(default)]
    pub(crate) selected_actions: Option<Vec<Action>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ThoughtProcessComponentRunResponse {
    pub(crate) component: String,
    pub(crate) mode: ThoughtProcessRunMode,
    pub(crate) output: Value,
    #[serde(default)]
    pub(crate) trace: Vec<ComponentTrace>,
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
