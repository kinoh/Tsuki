use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

use crate::clock::now_iso8601;
use crate::event::build_event;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig};
use crate::prompts::write_prompts;
use crate::{AppState, DebugImproveProposalRequest};

use super::improve_service::{
    ensure_active_submodule_exists, propose_improvement, replace_markdown_section_body,
    resolve_target_prompt_text, PromptTarget,
};

const TRIGGER_WORKER_SOURCE: &str = "self_improvement";
const TRIGGER_WORKER_MAX_CONCURRENCY: usize = 1;

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

pub(crate) fn spawn_trigger_worker(
    state: AppState,
    trigger_event_id: String,
    target: String,
    reason: String,
    feedback_refs: Vec<String>,
) {
    let semaphore = trigger_worker_semaphore();
    let permit = match semaphore.try_acquire_owned() {
        Ok(value) => value,
        Err(_) => {
            tokio::spawn(async move {
                emit_trigger_processed_event(
                    &state,
                    trigger_event_id.as_str(),
                    target.as_str(),
                    false,
                    false,
                    None,
                    "failed",
                    Some("TRIGGER_QUEUE_BUSY"),
                    Some("trigger worker concurrency limit reached"),
                )
                .await;
            });
            return;
        }
    };
    tokio::spawn(async move {
        let _permit = permit;
        run_trigger_worker(
            &state,
            trigger_event_id.as_str(),
            target.as_str(),
            reason.as_str(),
            &feedback_refs,
        )
        .await;
    });
}

fn trigger_worker_semaphore() -> Arc<Semaphore> {
    static TRIGGER_WORKER_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    TRIGGER_WORKER_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(TRIGGER_WORKER_MAX_CONCURRENCY)))
        .clone()
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

    emit_trigger_debug_raw(state, trigger_event_id, &input, &response).await;

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

    let status = decide_trigger_processed_status(
        issues.is_empty(),
        memory_updated,
        concept_graph_updated,
        proposal_id.is_some(),
    );
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

fn decide_trigger_processed_status(
    no_issue: bool,
    memory_updated: bool,
    concept_graph_updated: bool,
    proposal_created: bool,
) -> &'static str {
    if no_issue {
        return "success";
    }
    if memory_updated || concept_graph_updated || proposal_created {
        return "partial";
    }
    "failed"
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

async fn emit_trigger_debug_raw(
    state: &AppState,
    trigger_event_id: &str,
    input: &str,
    response: &crate::llm::LlmResponse,
) {
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

#[cfg(test)]
mod tests {
    use super::{
        decide_trigger_processed_status, normalize_trigger_target_for_prompt,
        parse_trigger_processing_plan,
    };

    #[test]
    fn parse_trigger_plan_accepts_plain_json() {
        let raw = r#"{
            "memory_section_update": {"target":"base","content":"updated"},
            "concept_upserts": ["submodule:curiosity"],
            "relation_additions": [{"from":"submodule:curiosity","to":"旅行","relation_type":"EVOKES"}],
            "proposal": {"target":"router","diff_text":"@@ -1 +1 @@\n-old\n+new"}
        }"#;
        let plan = parse_trigger_processing_plan(raw).expect("must parse");
        assert!(plan.memory_section_update.is_some());
        assert_eq!(plan.concept_upserts.len(), 1);
        assert_eq!(plan.relation_additions.len(), 1);
        assert!(plan.proposal.is_some());
    }

    #[test]
    fn parse_trigger_plan_accepts_wrapped_json() {
        let raw =
            "noise before\n{\"concept_upserts\":[\"a\"],\"relation_additions\":[]}\nnoise after";
        let plan = parse_trigger_processing_plan(raw).expect("must parse");
        assert_eq!(plan.concept_upserts, vec!["a".to_string()]);
    }

    #[test]
    fn parse_trigger_plan_rejects_invalid_json() {
        let raw = "no json object here";
        assert!(parse_trigger_processing_plan(raw).is_err());
    }

    #[test]
    fn normalize_trigger_target_falls_back_to_base() {
        assert_eq!(
            normalize_trigger_target_for_prompt("unknown-target"),
            "base"
        );
        assert_eq!(normalize_trigger_target_for_prompt("router"), "router");
    }

    #[test]
    fn decide_trigger_status_success_partial_failed() {
        assert_eq!(
            decide_trigger_processed_status(true, false, false, false),
            "success"
        );
        assert_eq!(
            decide_trigger_processed_status(false, true, false, false),
            "partial"
        );
        assert_eq!(
            decide_trigger_processed_status(false, false, true, false),
            "partial"
        );
        assert_eq!(
            decide_trigger_processed_status(false, false, false, true),
            "partial"
        );
        assert_eq!(
            decide_trigger_processed_status(false, false, false, false),
            "failed"
        );
    }
}
