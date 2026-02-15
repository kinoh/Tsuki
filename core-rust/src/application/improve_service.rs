use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::event::{build_event, Event};
use crate::module_registry::ModuleRegistryReader;
use crate::prompts::{write_prompts, PromptOverrides};
use crate::{
    AppState, DebugImproveProposalRequest, DebugImproveResponse, DebugImproveReviewRequest,
    DebugImproveTriggerRequest,
};

#[derive(Debug, Clone)]
enum PromptTarget {
    Base,
    Decision,
    Submodule(String),
}

impl PromptTarget {
    fn parse(raw: &str) -> Option<Self> {
        let value = raw.trim();
        if value.eq_ignore_ascii_case("base") {
            return Some(Self::Base);
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
        .unwrap_or_else(|| "debug trigger".to_string());
    let feedback_refs = payload.feedback_refs.unwrap_or_default();
    let trigger_event = build_event(
        "system",
        "text",
        json!({
            "phase": "trigger",
            "target": target,
            "reason": reason,
            "feedback_refs": feedback_refs,
        }),
        vec!["improve.trigger".to_string()],
    );
    crate::record_event(state, trigger_event.clone()).await;
    Ok(DebugImproveResponse {
        proposal_event_id: None,
        review_event_id: None,
        auto_approved: false,
        applied: false,
    })
}

pub(crate) async fn propose_improvement(
    state: &AppState,
    payload: DebugImproveProposalRequest,
) -> Result<DebugImproveResponse, (StatusCode, String)> {
    if PromptTarget::parse(&payload.target).is_none() {
        return Err((StatusCode::BAD_REQUEST, "invalid target".to_string()));
    }
    if payload.section.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "section is required".to_string()));
    }
    let proposal_event = build_event(
        "system",
        "text",
        json!({
            "phase": "proposal",
            "target": payload.target.trim(),
            "section": payload.section.trim(),
            "reason": payload.reason.clone().unwrap_or_else(|| "none".to_string()),
            "content": payload.content,
            "status": "pending",
            "feedback_refs": payload.feedback_refs.unwrap_or_default(),
        }),
        vec!["improve.proposal".to_string()],
    );
    crate::record_event(state, proposal_event.clone()).await;

    let is_memory = proposal_event
        .payload
        .get("section")
        .and_then(|value| value.as_str())
        .map(|value| value == "Memory")
        .unwrap_or(false);
    if !is_memory {
        return Ok(DebugImproveResponse {
            proposal_event_id: Some(proposal_event.event_id),
            review_event_id: None,
            auto_approved: false,
            applied: false,
        });
    }

    let review_event = build_event(
        "system",
        "text",
        json!({
            "phase": "review",
            "proposal_event_id": proposal_event.event_id.clone(),
            "review": "approval",
            "reason": "auto_approval:Memory",
            "status": "approved",
        }),
        vec!["improve.review".to_string()],
    );
    crate::record_event(state, review_event.clone()).await;

    let apply_result = apply_projection(state, &proposal_event).await;
    if let Err(err) = apply_result {
        emit_projection_error(state, review_event.event_id.as_str(), &err).await;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, err));
    }
    Ok(DebugImproveResponse {
        proposal_event_id: Some(proposal_event.event_id),
        review_event_id: Some(review_event.event_id),
        auto_approved: true,
        applied: true,
    })
}

pub(crate) async fn review_improvement(
    state: &AppState,
    payload: DebugImproveReviewRequest,
) -> Result<DebugImproveResponse, (StatusCode, String)> {
    let proposal_event = state
        .event_store
        .get_by_id(payload.proposal_event_id.as_str())
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
        .any(|tag| tag == "improve.proposal")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "event is not improve.proposal".to_string(),
        ));
    }
    let review = normalize_review_value(payload.review.as_str()).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "review must be approval or rejection".to_string(),
        )
    })?;
    let status = if review == "approval" {
        "approved"
    } else {
        "rejected"
    };
    let review_reason = payload.reason.clone().unwrap_or_else(|| "none".to_string());
    let review_event = build_event(
        "system",
        "text",
        json!({
            "phase": "review",
            "proposal_event_id": proposal_event.event_id.clone(),
            "review": review,
            "reason": review_reason,
            "status": status,
        }),
        vec!["improve.review".to_string()],
    );
    crate::record_event(state, review_event.clone()).await;

    if review != "approval" {
        return Ok(DebugImproveResponse {
            proposal_event_id: Some(proposal_event.event_id),
            review_event_id: Some(review_event.event_id),
            auto_approved: false,
            applied: false,
        });
    }

    let apply_result = apply_projection(state, &proposal_event).await;
    if let Err(err) = apply_result {
        emit_projection_error(state, review_event.event_id.as_str(), &err).await;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, err));
    }
    Ok(DebugImproveResponse {
        proposal_event_id: Some(proposal_event.event_id),
        review_event_id: Some(review_event.event_id),
        auto_approved: false,
        applied: true,
    })
}

fn normalize_review_value(value: &str) -> Option<&'static str> {
    if value.eq_ignore_ascii_case("approval") || value.eq_ignore_ascii_case("approved") {
        return Some("approval");
    }
    if value.eq_ignore_ascii_case("rejection") || value.eq_ignore_ascii_case("rejected") {
        return Some("rejection");
    }
    None
}

async fn emit_projection_error(state: &AppState, review_event_id: &str, err: &str) {
    let error_event = build_event(
        "system",
        "text",
        json!({
            "event_id": review_event_id,
            "text": format!("projection failed: {}", err),
        }),
        vec!["error".to_string()],
    );
    crate::record_event(state, error_event).await;
}

async fn apply_projection(state: &AppState, proposal_event: &Event) -> Result<(), String> {
    let target_raw = payload_str(&proposal_event.payload, "target")
        .ok_or_else(|| "proposal target is required".to_string())?;
    let section = payload_str(&proposal_event.payload, "section")
        .ok_or_else(|| "proposal section is required".to_string())?;
    let content = payload_str(&proposal_event.payload, "content")
        .ok_or_else(|| "proposal content is required".to_string())?;
    let target = PromptTarget::parse(target_raw.as_str())
        .ok_or_else(|| format!("invalid proposal target: {}", target_raw))?;

    let mut overrides = state.prompts.read().await.clone();
    let current_target_prompt = resolve_target_prompt_text(state, &overrides, &target).await?;
    let next_target_prompt = if section == "Memory" {
        replace_markdown_section_body(current_target_prompt.as_str(), "Memory", content.as_str())?
    } else {
        content.to_string()
    };

    match &target {
        PromptTarget::Base => {
            overrides.base = Some(next_target_prompt);
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
