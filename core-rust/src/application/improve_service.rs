use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::clock::now_iso8601;
use crate::event::build_event;
use crate::module_registry::ModuleRegistryReader;
use crate::prompts::{write_prompts, PromptOverrides};
use crate::{
    AppState, DebugImproveProposalRequest, DebugImproveResponse, DebugImproveReviewRequest,
    DebugImproveTriggerRequest,
};

const MAX_REVIEW_SCAN_EVENTS: usize = 5000;
#[derive(Debug, Clone)]
pub(crate) enum PromptTarget {
    Base,
    Router,
    Decision,
    Submodule(String),
}

impl PromptTarget {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
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

    crate::application::self_improvement_trigger_service::spawn_trigger_worker(
        state.clone(),
        trigger_event_id,
        target,
        reason,
        feedback_refs,
    );

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

pub(crate) async fn ensure_active_submodule_exists(
    state: &AppState,
    name: &str,
) -> Result<(), String> {
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

pub(crate) async fn resolve_target_prompt_text(
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

pub(crate) fn payload_str(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

pub(crate) fn replace_markdown_section_body(
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
