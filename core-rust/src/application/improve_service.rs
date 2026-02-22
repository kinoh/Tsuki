use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::clock::now_iso8601;
use crate::event::build_event;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig};
use crate::module_registry::ModuleRegistryReader;
use crate::prompts::{write_prompts, PromptOverrides};
use crate::{
    AppState, DebugImproveProposalRequest, DebugImproveResponse, DebugImproveReviewRequest,
    DebugImproveTriggerRequest,
};

const MAX_REVIEW_SCAN_EVENTS: usize = 5000;
const TRIGGER_WORKER_SOURCE: &str = "self_improvement";

#[derive(Debug, Clone)]
enum PromptTarget {
    Base,
    Router,
    Decision,
    Submodule(String),
}

impl PromptTarget {
    fn parse(raw: &str) -> Option<Self> {
        let value = raw.trim();
        if value.eq_ignore_ascii_case("base") {
            return Some(Self::Base);
        }
        if value.eq_ignore_ascii_case("router") {
            return Some(Self::Router);
        }
        if value.eq_ignore_ascii_case("decision") {
            return Some(Self::Decision);
        }
        let prefix = "submodule:";
        if let Some(head) = value.get(..prefix.len()) {
            if head.eq_ignore_ascii_case(prefix) {
                let name = value.get(prefix.len()..).unwrap_or("").trim();
                if !name.is_empty() {
                    return Some(Self::Submodule(name.to_string()));
                }
            }
        }
        None
    }
}

pub(crate) async fn trigger_improvement(
    state: &AppState,
    payload: DebugImproveTriggerRequest,
) -> Result<DebugImproveResponse, (StatusCode, String)> {
    let target = payload
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual")
        .to_string();
    let reason = payload
        .reason
        .unwrap_or_else(|| "manual trigger".to_string());
    let feedback_refs = payload.feedback_refs.unwrap_or_default();
    let trigger_event = build_event(
        "system",
        "text",
        json!({
            "target": target,
            "reason": reason,
            "feedback_refs": feedback_refs,
            "created_at": now_iso8601(),
        }),
        vec!["self_improvement.triggered".to_string()],
    );
    let trigger_event_id = trigger_event.event_id.clone();
    crate::record_event(state, trigger_event).await;

    let worker_state = state.clone();
    let worker_target = target.clone();
    let worker_reason = reason.clone();
    let worker_feedback_refs = feedback_refs.clone();
    tokio::spawn(async move {
        run_trigger_worker(
            &worker_state,
            trigger_event_id.as_str(),
            worker_target.as_str(),
            worker_reason.as_str(),
            &worker_feedback_refs,
        )
        .await;
    });

    Ok(DebugImproveResponse {
        proposal_id: None,
        review_event_id: None,
        apply_event_id: None,
        applied: false,
    })
}

pub(crate) async fn propose_improvement(
    state: &AppState,
    payload: DebugImproveProposalRequest,
) -> Result<DebugImproveResponse, (StatusCode, String)> {
    let target_raw = payload.target.trim();
    if PromptTarget::parse(target_raw).is_none() {
        return Err((StatusCode::BAD_REQUEST, "invalid target".to_string()));
    }

    let job_id = payload.job_id.trim();
    if job_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "job_id is required".to_string()));
    }

    let diff_text = payload.diff_text.as_str();
    if diff_text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "diff_text is required".to_string()));
    }

    if matches!(payload.requires_approval, Some(false)) {
        return Err((
            StatusCode::BAD_REQUEST,
            "requires_approval must be true".to_string(),
        ));
    }

    validate_unified_diff(diff_text).map_err(|err| (StatusCode::BAD_REQUEST, err))?;

    let created_by = payload
        .created_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual")
        .to_string();
    let created_at = now_iso8601();
    let mut proposal_event = build_event(
        "system",
        "text",
        json!({
            "proposal_id": "",
            "job_id": job_id,
            "target": target_raw,
            "diff_text": diff_text,
            "requires_approval": true,
            "created_by": created_by,
            "created_at": created_at,
        }),
        vec!["self_improvement.proposed".to_string()],
    );

    let proposal_id = proposal_event.event_id.clone();
    proposal_event.payload["proposal_id"] = json!(proposal_id);

    crate::record_event(state, proposal_event).await;

    Ok(DebugImproveResponse {
        proposal_id: Some(proposal_id),
        review_event_id: None,
        apply_event_id: None,
        applied: false,
    })
}

pub(crate) async fn review_improvement(
    state: &AppState,
    payload: DebugImproveReviewRequest,
) -> Result<DebugImproveResponse, (StatusCode, String)> {
    let proposal_id = payload.proposal_id.trim();
    if proposal_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "proposal_id is required".to_string(),
        ));
    }

    let job_id = payload.job_id.trim();
    if job_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "job_id is required".to_string()));
    }

    let target_raw = payload.target.trim();
    let target = PromptTarget::parse(target_raw)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "invalid target".to_string()))?;

    if proposal_has_review(state, proposal_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?
    {
        return Err((
            StatusCode::CONFLICT,
            "review already exists for proposal_id".to_string(),
        ));
    }

    let proposal_event = state
        .event_store
        .get_by_id(proposal_id)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "proposal event not found".to_string(),
            )
        })?;
    if !proposal_event
        .meta
        .tags
        .iter()
        .any(|tag| tag == "self_improvement.proposed")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "event is not self_improvement.proposed".to_string(),
        ));
    }

    let proposal_job_id = payload_str(&proposal_event.payload, "job_id").ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "proposal job_id is missing".to_string(),
        )
    })?;
    if proposal_job_id != job_id {
        return Err((StatusCode::BAD_REQUEST, "job_id mismatch".to_string()));
    }

    let proposal_target = payload_str(&proposal_event.payload, "target").ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "proposal target is missing".to_string(),
        )
    })?;
    if !proposal_target.eq_ignore_ascii_case(target_raw) {
        return Err((StatusCode::BAD_REQUEST, "target mismatch".to_string()));
    }

    let decision = normalize_decision(payload.decision.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "decision must be approved or rejected".to_string(),
        )
    })?;

    let reviewed_by = payload
        .reviewed_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("manual")
        .to_string();
    let review_reason = payload
        .review_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("none")
        .to_string();
    let reviewed_at = now_iso8601();
    let review_event = build_event(
        "system",
        "text",
        json!({
            "proposal_id": proposal_id,
            "job_id": job_id,
            "target": target_raw,
            "decision": decision,
            "reviewed_by": reviewed_by,
            "review_reason": review_reason,
            "reviewed_at": reviewed_at,
        }),
        vec!["self_improvement.reviewed".to_string()],
    );
    let review_event_id = review_event.event_id.clone();
    crate::record_event(state, review_event).await;

    if decision != "approved" {
        return Ok(DebugImproveResponse {
            proposal_id: Some(proposal_id.to_string()),
            review_event_id: Some(review_event_id),
            apply_event_id: None,
            applied: false,
        });
    }

    let applied_at = now_iso8601();
    let applied_by = "runtime";
    match apply_prompt_diff(state, &target, &proposal_event)
        .await
        .map(|_| payload_str(&proposal_event.payload, "diff_text").unwrap_or_default())
    {
        Ok(applied_diff_text) => {
            let apply_event = build_event(
                "system",
                "text",
                json!({
                    "proposal_id": proposal_id,
                    "job_id": job_id,
                    "target": target_raw,
                    "status": "success",
                    "applied_by": applied_by,
                    "applied_at": applied_at,
                    "applied_diff_text": applied_diff_text,
                }),
                vec!["self_improvement.applied".to_string()],
            );
            let apply_event_id = apply_event.event_id.clone();
            crate::record_event(state, apply_event).await;
            Ok(DebugImproveResponse {
                proposal_id: Some(proposal_id.to_string()),
                review_event_id: Some(review_event_id),
                apply_event_id: Some(apply_event_id),
                applied: true,
            })
        }
        Err(err) => {
            let apply_event = build_event(
                "system",
                "text",
                json!({
                    "proposal_id": proposal_id,
                    "job_id": job_id,
                    "target": target_raw,
                    "status": "failed",
                    "applied_by": applied_by,
                    "applied_at": applied_at,
                    "error_code": "APPLY_DIFF_FAILED",
                    "error_detail": err,
                }),
                vec!["self_improvement.applied".to_string()],
            );
            let apply_event_id = apply_event.event_id.clone();
            crate::record_event(state, apply_event).await;
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("apply failed (event_id={})", apply_event_id),
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
struct TriggerProcessingPlan {
    #[serde(default)]
    memory_section_update: Option<MemorySectionUpdatePlan>,
    #[serde(default)]
    concept_upserts: Vec<String>,
    #[serde(default)]
    relation_additions: Vec<RelationAdditionPlan>,
    #[serde(default)]
    proposal: Option<TriggerProposalPlan>,
}

#[derive(Debug, Deserialize)]
struct MemorySectionUpdatePlan {
    #[serde(default)]
    target: Option<String>,
    content: String,
}

#[derive(Debug, Deserialize)]
struct RelationAdditionPlan {
    from: String,
    to: String,
    relation_type: String,
}

#[derive(Debug, Deserialize)]
struct TriggerProposalPlan {
    #[serde(default)]
    target: Option<String>,
    diff_text: String,
}

#[derive(Debug)]
struct TriggerWorkerIssue {
    code: &'static str,
    detail: String,
}

async fn run_trigger_worker(
    state: &AppState,
    trigger_event_id: &str,
    target: &str,
    reason: &str,
    feedback_refs: &[String],
) {
    let input = json!({
        "trigger_event_id": trigger_event_id,
        "target": target,
        "reason": reason,
        "feedback_refs": feedback_refs,
    })
    .to_string();
    let adapter = ResponseApiAdapter::new(ResponseApiConfig {
        model: state.modules.runtime.model.clone(),
        instructions: trigger_worker_instructions(),
        temperature: state.modules.runtime.temperature,
        max_output_tokens: state.modules.runtime.max_output_tokens,
        tools: Vec::new(),
        tool_handler: None,
        max_tool_rounds: 0,
    });

    let response = match adapter
        .respond(LlmRequest {
            input: input.clone(),
        })
        .await
    {
        Ok(value) => value,
        Err(err) => {
            emit_trigger_processed_event(
                state,
                trigger_event_id,
                target,
                false,
                false,
                None,
                "failed",
                Some("TRIGGER_LLM_CALL_FAILED"),
                Some(err.to_string().as_str()),
            )
            .await;
            return;
        }
    };

    maybe_emit_trigger_debug_raw(state, trigger_event_id, &input, &response).await;

    let plan = match parse_trigger_processing_plan(response.text.as_str()) {
        Ok(value) => value,
        Err(err) => {
            emit_trigger_processed_event(
                state,
                trigger_event_id,
                target,
                false,
                false,
                None,
                "failed",
                Some("TRIGGER_PLAN_PARSE_FAILED"),
                Some(err.as_str()),
            )
            .await;
            return;
        }
    };

    let mut memory_updated = false;
    let mut concept_graph_updated = false;
    let mut proposal_id = None::<String>;
    let mut issues = Vec::<TriggerWorkerIssue>::new();

    if let Some(memory_plan) = plan.memory_section_update {
        match apply_memory_section_update(state, target, &memory_plan).await {
            Ok(updated) => {
                memory_updated = updated;
            }
            Err(err) => issues.push(TriggerWorkerIssue {
                code: "MEMORY_UPDATE_FAILED",
                detail: err,
            }),
        }
    }

    for concept in plan.concept_upserts {
        let name = concept.trim();
        if name.is_empty() {
            continue;
        }
        match state
            .activation_concept_graph
            .concept_upsert(name.to_string())
            .await
        {
            Ok(_) => {
                concept_graph_updated = true;
            }
            Err(err) => issues.push(TriggerWorkerIssue {
                code: "CONCEPT_UPSERT_FAILED",
                detail: err,
            }),
        }
    }

    for relation in plan.relation_additions {
        if relation.from.trim().is_empty()
            || relation.to.trim().is_empty()
            || relation.relation_type.trim().is_empty()
        {
            continue;
        }
        match state
            .activation_concept_graph
            .relation_add(relation.from, relation.to, relation.relation_type)
            .await
        {
            Ok(_) => {
                concept_graph_updated = true;
            }
            Err(err) => issues.push(TriggerWorkerIssue {
                code: "RELATION_ADD_FAILED",
                detail: err,
            }),
        }
    }

    if let Some(proposal) = plan.proposal {
        let proposal_target = proposal
            .target
            .as_deref()
            .map(str::trim)
            .filter(|value| PromptTarget::parse(value).is_some())
            .map(str::to_string)
            .unwrap_or_else(|| normalize_trigger_target_for_prompt(target));
        match propose_improvement(
            state,
            DebugImproveProposalRequest {
                target: proposal_target,
                job_id: format!("trigger:{}", trigger_event_id),
                diff_text: proposal.diff_text,
                requires_approval: Some(true),
                created_by: Some(TRIGGER_WORKER_SOURCE.to_string()),
            },
        )
        .await
        {
            Ok(result) => {
                proposal_id = result.proposal_id;
            }
            Err((_, err)) => issues.push(TriggerWorkerIssue {
                code: "PROPOSAL_CREATE_FAILED",
                detail: err,
            }),
        }
    }

    let status = if issues.is_empty() {
        "success"
    } else if memory_updated || concept_graph_updated || proposal_id.is_some() {
        "partial"
    } else {
        "failed"
    };
    let first_issue = issues.first();
    let error_code = first_issue.map(|item| item.code);
    let error_detail = if issues.is_empty() {
        None
    } else {
        Some(
            issues
                .iter()
                .map(|issue| format!("{}: {}", issue.code, issue.detail))
                .collect::<Vec<_>>()
                .join(" | "),
        )
    };

    emit_trigger_processed_event(
        state,
        trigger_event_id,
        target,
        memory_updated,
        concept_graph_updated,
        proposal_id.as_deref(),
        status,
        error_code,
        error_detail.as_deref(),
    )
    .await;
}

fn trigger_worker_instructions() -> String {
    [
        "You are the self-improvement trigger worker.",
        "Read JSON input and return one JSON object only.",
        "Do not include markdown or explanations.",
        "Schema:",
        "{",
        "  \"memory_section_update\": {\"target\": \"base|router|decision|submodule:<name>\", \"content\": \"...\"} | null,",
        "  \"concept_upserts\": [\"concept_name\", ...],",
        "  \"relation_additions\": [{\"from\": \"...\", \"to\": \"...\", \"relation_type\": \"IS_A|PART_OF|EVOKES\"}, ...],",
        "  \"proposal\": {\"target\": \"base|router|decision|submodule:<name>\", \"diff_text\": \"unified diff text\"} | null",
        "}",
        "If there is not enough signal, return null/empty fields instead of guessing.",
    ]
    .join("\n")
}

fn parse_trigger_processing_plan(text: &str) -> Result<TriggerProcessingPlan, String> {
    if let Ok(value) = serde_json::from_str::<TriggerProcessingPlan>(text) {
        return Ok(value);
    }
    let start = text
        .find('{')
        .ok_or_else(|| "trigger plan is not a JSON object".to_string())?;
    let end = text
        .rfind('}')
        .ok_or_else(|| "trigger plan is not a JSON object".to_string())?;
    let candidate = text
        .get(start..=end)
        .ok_or_else(|| "failed to slice trigger plan JSON".to_string())?;
    serde_json::from_str::<TriggerProcessingPlan>(candidate)
        .map_err(|err| format!("invalid trigger plan JSON: {}", err))
}

fn normalize_trigger_target_for_prompt(target: &str) -> String {
    let trimmed = target.trim();
    if PromptTarget::parse(trimmed).is_some() {
        return trimmed.to_string();
    }
    "base".to_string()
}

async fn maybe_emit_trigger_debug_raw(
    state: &AppState,
    trigger_event_id: &str,
    input: &str,
    response: &crate::llm::LlmResponse,
) {
    let enabled = std::env::var("SELF_IMPROVEMENT_EMIT_LLM_RAW")
        .map(|value| value == "1")
        .unwrap_or(false);
    if !enabled {
        return;
    }
    let event = build_event(
        TRIGGER_WORKER_SOURCE,
        "text",
        json!({
            "trigger_event_id": trigger_event_id,
            "input": input,
            "output_text": response.text,
            "raw": response.raw,
            "tool_calls": response.tool_calls,
        }),
        vec![
            "debug".to_string(),
            "llm.raw".to_string(),
            "module:self_improvement_trigger".to_string(),
        ],
    );
    crate::record_event(state, event).await;
}

async fn emit_trigger_processed_event(
    state: &AppState,
    trigger_event_id: &str,
    target: &str,
    memory_updated: bool,
    concept_graph_updated: bool,
    proposal_id: Option<&str>,
    status: &str,
    error_code: Option<&str>,
    error_detail: Option<&str>,
) {
    let mut payload = json!({
        "trigger_event_id": trigger_event_id,
        "target": target,
        "status": status,
        "memory_updated": memory_updated,
        "concept_graph_updated": concept_graph_updated,
        "processed_at": now_iso8601(),
    });
    if let Some(value) = proposal_id {
        payload["proposal_id"] = json!(value);
    }
    if let Some(value) = error_code {
        payload["error_code"] = json!(value);
    }
    if let Some(value) = error_detail {
        payload["error_detail"] = json!(value);
    }
    let event = build_event(
        TRIGGER_WORKER_SOURCE,
        "text",
        payload,
        vec!["self_improvement.trigger_processed".to_string()],
    );
    crate::record_event(state, event).await;
}

async fn apply_memory_section_update(
    state: &AppState,
    fallback_target: &str,
    memory_plan: &MemorySectionUpdatePlan,
) -> Result<bool, String> {
    let content = memory_plan.content.trim();
    if content.is_empty() {
        return Ok(false);
    }

    let target_raw = memory_plan
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_target);
    let target = PromptTarget::parse(target_raw).unwrap_or(PromptTarget::Base);

    let mut overrides = state.prompts.read().await.clone();
    let current = resolve_target_prompt_text(state, &overrides, &target).await?;
    let next = replace_markdown_section_body(current.as_str(), "Memory", content)?;

    match &target {
        PromptTarget::Base => {
            overrides.base = Some(next);
        }
        PromptTarget::Router => {
            overrides.router = Some(next);
        }
        PromptTarget::Decision => {
            overrides.decision = Some(next);
        }
        PromptTarget::Submodule(name) => {
            ensure_active_submodule_exists(state, name).await?;
            overrides.submodules.insert(name.clone(), next);
        }
    }

    write_prompts(&state.prompts_path, &overrides)?;
    *state.prompts.write().await = overrides;
    Ok(true)
}

fn normalize_decision(value: &str) -> Option<&'static str> {
    if value.eq_ignore_ascii_case("approved") || value.eq_ignore_ascii_case("approval") {
        return Some("approved");
    }
    if value.eq_ignore_ascii_case("rejected") || value.eq_ignore_ascii_case("rejection") {
        return Some("rejected");
    }
    None
}

async fn proposal_has_review(state: &AppState, proposal_id: &str) -> Result<bool, String> {
    let events = state
        .event_store
        .latest(MAX_REVIEW_SCAN_EVENTS)
        .await
        .map_err(|err| err.to_string())?;
    Ok(events.into_iter().any(|event| {
        event
            .meta
            .tags
            .iter()
            .any(|tag| tag == "self_improvement.reviewed")
            && payload_str(&event.payload, "proposal_id")
                .map(|id| id == proposal_id)
                .unwrap_or(false)
    }))
}

async fn apply_prompt_diff(
    state: &AppState,
    target: &PromptTarget,
    proposal_event: &crate::event::Event,
) -> Result<(), String> {
    let diff_text = payload_str(&proposal_event.payload, "diff_text")
        .ok_or_else(|| "proposal diff_text is required".to_string())?;

    let mut overrides = state.prompts.read().await.clone();
    let current_target_prompt = resolve_target_prompt_text(state, &overrides, target).await?;
    let next_target_prompt =
        apply_unified_diff(current_target_prompt.as_str(), diff_text.as_str())?;

    match target {
        PromptTarget::Base => {
            overrides.base = Some(next_target_prompt);
        }
        PromptTarget::Router => {
            overrides.router = Some(next_target_prompt);
        }
        PromptTarget::Decision => {
            overrides.decision = Some(next_target_prompt);
        }
        PromptTarget::Submodule(name) => {
            ensure_active_submodule_exists(state, name).await?;
            overrides
                .submodules
                .insert(name.clone(), next_target_prompt);
        }
    }

    write_prompts(&state.prompts_path, &overrides)?;
    *state.prompts.write().await = overrides;
    Ok(())
}

async fn ensure_active_submodule_exists(state: &AppState, name: &str) -> Result<(), String> {
    let modules = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| err.to_string())?;
    if modules.iter().any(|module| module.name == name) {
        return Ok(());
    }
    Err(format!("submodule not found: {}", name))
}

async fn resolve_target_prompt_text(
    state: &AppState,
    overrides: &PromptOverrides,
    target: &PromptTarget,
) -> Result<String, String> {
    match target {
        PromptTarget::Base => Ok(overrides
            .base
            .clone()
            .unwrap_or_else(|| state.modules.runtime.base_instructions.clone())),
        PromptTarget::Router => Ok(overrides
            .router
            .clone()
            .unwrap_or_else(|| state.router_instructions.clone())),
        PromptTarget::Decision => Ok(overrides
            .decision
            .clone()
            .unwrap_or_else(|| state.decision_instructions.clone())),
        PromptTarget::Submodule(name) => {
            if let Some(text) = overrides.submodules.get(name) {
                return Ok(text.clone());
            }
            let module = state
                .modules
                .registry
                .list_active()
                .await
                .map_err(|err| err.to_string())?
                .into_iter()
                .find(|item| item.name == *name)
                .ok_or_else(|| format!("submodule not found: {}", name))?;
            Ok(module.instructions)
        }
    }
}

fn payload_str(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

fn replace_markdown_section_body(
    source: &str,
    section_name: &str,
    body: &str,
) -> Result<String, String> {
    let (start, end) = find_markdown_section_body_range(source, section_name)
        .ok_or_else(|| format!("section not found: {}", section_name))?;
    let mut replacement = body.trim_end_matches('\n').to_string();
    replacement.push('\n');
    let mut output = String::with_capacity(source.len() + replacement.len());
    output.push_str(&source[..start]);
    output.push_str(&replacement);
    output.push_str(&source[end..]);
    Ok(output)
}

fn find_markdown_section_body_range(source: &str, section_name: &str) -> Option<(usize, usize)> {
    #[derive(Clone, Copy)]
    struct Heading {
        level: usize,
        line_start: usize,
        body_start: usize,
    }

    let mut headings: Vec<(Heading, String)> = Vec::new();
    let mut offset = 0usize;
    for line in source.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        let hash_count = content.chars().take_while(|ch| *ch == '#').count();
        if hash_count == 0 {
            continue;
        }
        if content.chars().nth(hash_count) != Some(' ') {
            continue;
        }
        let title = content[hash_count + 1..].trim().to_string();
        headings.push((
            Heading {
                level: hash_count,
                line_start,
                body_start: offset,
            },
            title,
        ));
    }
    for (index, (heading, title)) in headings.iter().enumerate() {
        if title != section_name {
            continue;
        }
        let mut end = source.len();
        for (next, _) in headings.iter().skip(index + 1) {
            if next.level <= heading.level {
                end = next.line_start;
                break;
            }
        }
        return Some((heading.body_start, end));
    }
    None
}

#[derive(Debug)]
struct UnifiedDiffHunk {
    old_start: usize,
    old_count: usize,
    new_count: usize,
    lines: Vec<UnifiedDiffLine>,
}

#[derive(Debug)]
enum UnifiedDiffLine {
    Context(String),
    Add(String),
    Remove(String),
}

fn validate_unified_diff(diff_text: &str) -> Result<(), String> {
    parse_unified_diff(diff_text).map(|_| ())
}

fn apply_unified_diff(source: &str, diff_text: &str) -> Result<String, String> {
    let hunks = parse_unified_diff(diff_text)?;

    let source_has_trailing_newline = source.ends_with('\n');
    let source_core = source.strip_suffix('\n').unwrap_or(source);
    let source_lines = if source_core.is_empty() {
        Vec::<String>::new()
    } else {
        source_core
            .split('\n')
            .map(str::to_string)
            .collect::<Vec<_>>()
    };

    let mut output = Vec::<String>::new();
    let mut source_cursor = 0usize;

    for hunk in hunks {
        let old_start_index = hunk.old_start.saturating_sub(1);
        if old_start_index < source_cursor {
            return Err("invalid diff: overlapping hunks".to_string());
        }
        if old_start_index > source_lines.len() {
            return Err("invalid diff: hunk start out of range".to_string());
        }

        output.extend(source_lines[source_cursor..old_start_index].iter().cloned());
        source_cursor = old_start_index;

        let mut old_consumed = 0usize;
        let mut new_produced = 0usize;

        for line in hunk.lines {
            match line {
                UnifiedDiffLine::Context(text) => {
                    let current = source_lines
                        .get(source_cursor)
                        .ok_or_else(|| "invalid diff: context out of range".to_string())?;
                    if current != &text {
                        return Err("invalid diff: context line mismatch".to_string());
                    }
                    output.push(text);
                    source_cursor += 1;
                    old_consumed += 1;
                    new_produced += 1;
                }
                UnifiedDiffLine::Remove(text) => {
                    let current = source_lines
                        .get(source_cursor)
                        .ok_or_else(|| "invalid diff: removal out of range".to_string())?;
                    if current != &text {
                        return Err("invalid diff: removed line mismatch".to_string());
                    }
                    source_cursor += 1;
                    old_consumed += 1;
                }
                UnifiedDiffLine::Add(text) => {
                    output.push(text);
                    new_produced += 1;
                }
            }
        }

        if old_consumed != hunk.old_count {
            return Err("invalid diff: old line count mismatch".to_string());
        }
        if new_produced != hunk.new_count {
            return Err("invalid diff: new line count mismatch".to_string());
        }
    }

    output.extend(source_lines[source_cursor..].iter().cloned());

    let mut result = output.join("\n");
    if source_has_trailing_newline {
        result.push('\n');
    }
    Ok(result)
}

fn parse_unified_diff(diff_text: &str) -> Result<Vec<UnifiedDiffHunk>, String> {
    let mut hunks = Vec::<UnifiedDiffHunk>::new();
    let mut current_hunk: Option<UnifiedDiffHunk> = None;

    for line in diff_text.lines() {
        if line.starts_with("@@") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            current_hunk = Some(parse_unified_hunk_header(line)?);
            continue;
        }

        if current_hunk.is_none() {
            if line.starts_with("--- ")
                || line.starts_with("+++ ")
                || line.starts_with("diff ")
                || line.starts_with("index ")
            {
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            return Err("invalid diff: missing hunk header".to_string());
        }

        if line == "\\ No newline at end of file" {
            continue;
        }

        let Some(hunk) = current_hunk.as_mut() else {
            return Err("invalid diff: missing hunk".to_string());
        };

        let mut chars = line.chars();
        let op = chars
            .next()
            .ok_or_else(|| "invalid diff: empty hunk line".to_string())?;
        let text = chars.collect::<String>();

        match op {
            ' ' => hunk.lines.push(UnifiedDiffLine::Context(text)),
            '+' => hunk.lines.push(UnifiedDiffLine::Add(text)),
            '-' => hunk.lines.push(UnifiedDiffLine::Remove(text)),
            _ => {
                return Err("invalid diff: unsupported hunk line prefix".to_string());
            }
        }
    }

    if let Some(hunk) = current_hunk.take() {
        hunks.push(hunk);
    }

    if hunks.is_empty() {
        return Err("invalid diff: no hunks".to_string());
    }

    Ok(hunks)
}

fn parse_unified_hunk_header(line: &str) -> Result<UnifiedDiffHunk, String> {
    let Some(inner_start) = line.strip_prefix("@@") else {
        return Err("invalid diff: malformed hunk header".to_string());
    };
    let Some((ranges, _)) = inner_start.split_once("@@") else {
        return Err("invalid diff: malformed hunk header".to_string());
    };

    let mut parts = ranges.split_whitespace();
    let old_range = parts
        .next()
        .ok_or_else(|| "invalid diff: missing old range".to_string())?;
    let new_range = parts
        .next()
        .ok_or_else(|| "invalid diff: missing new range".to_string())?;

    let (old_start, old_count) = parse_hunk_range(old_range, '-')?;
    let (_new_start, new_count) = parse_hunk_range(new_range, '+')?;

    Ok(UnifiedDiffHunk {
        old_start,
        old_count,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_hunk_range(range: &str, expected_prefix: char) -> Result<(usize, usize), String> {
    let Some(raw) = range.strip_prefix(expected_prefix) else {
        return Err("invalid diff: malformed hunk range".to_string());
    };

    let (start_raw, count_raw) = match raw.split_once(',') {
        Some((start, count)) => (start, Some(count)),
        None => (raw, None),
    };

    let start = start_raw
        .parse::<usize>()
        .map_err(|_| "invalid diff: invalid range start".to_string())?;
    let count = count_raw
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| "invalid diff: invalid range count".to_string())
        })
        .transpose()?
        .unwrap_or(1);

    Ok((start, count))
}
