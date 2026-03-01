use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use tokio::sync::{broadcast::error::RecvError, Semaphore};

use crate::application::history_service::format_event_history;
use crate::clock::now_iso8601;
use crate::event::build_event;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig};
use crate::module_registry::ModuleRegistryReader;
use crate::prompts::write_prompts;
use crate::{AppState, DebugImproveProposalRequest};

use super::improve_approval_service::{
    ensure_active_submodule_exists, propose_improvement, replace_markdown_section_body,
    resolve_target_prompt_text, PromptTarget,
};

const TRIGGER_WORKER_SOURCE: &str = "self_improvement";
const TRIGGER_WORKER_MAX_CONCURRENCY: usize = 1;

pub(crate) fn start_trigger_consumer(state: AppState) {
    tokio::spawn(async move {
        let mut rx = state.tx.subscribe();
        loop {
            let event = match rx.recv().await {
                Ok(value) => value,
                Err(RecvError::Lagged(skipped)) => {
                    println!("TRIGGER_CONSUMER_LAGGED skipped={}", skipped);
                    continue;
                }
                Err(RecvError::Closed) => break,
            };
            if !event
                .meta
                .tags
                .iter()
                .any(|tag| tag == "self_improvement.triggered")
            {
                continue;
            }

            let target = payload_str_or(event.payload.get("target"), "all");
            let reason = payload_str_or(event.payload.get("reason"), "manual trigger");
            spawn_trigger_worker(state.clone(), event.event_id.clone(), target, reason);
        }
    });
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

#[derive(Debug)]
struct ModuleProcessResult {
    module_target: String,
    status: &'static str,
    memory_updated: bool,
    concept_graph_updated: bool,
    concept_ensured: Option<bool>,
    proposal_id: Option<String>,
    error_code: Option<&'static str>,
    error_detail: Option<String>,
}

pub(crate) fn spawn_trigger_worker(
    state: AppState,
    trigger_event_id: String,
    target: String,
    reason: String,
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
                    &[],
                    &[],
                    false,
                    false,
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
        run_trigger_orchestrator(
            &state,
            trigger_event_id.as_str(),
            target.as_str(),
            reason.as_str(),
        )
        .await;
    });
}

async fn run_trigger_orchestrator(
    state: &AppState,
    trigger_event_id: &str,
    trigger_target: &str,
    reason: &str,
) {
    let module_targets = match resolve_trigger_targets(state, trigger_target).await {
        Ok(value) => value,
        Err(err) => {
            emit_trigger_processed_event(
                state,
                trigger_event_id,
                trigger_target,
                &[],
                &[],
                false,
                false,
                "failed",
                Some("TRIGGER_TARGET_RESOLVE_FAILED"),
                Some(err.as_str()),
            )
            .await;
            return;
        }
    };

    let mut results = Vec::<ModuleProcessResult>::new();
    for module_target in &module_targets {
        let result =
            run_module_worker(state, trigger_event_id, module_target.as_str(), reason).await;
        emit_module_processed_event(state, trigger_event_id, &result).await;
        results.push(result);
    }

    let proposal_ids = results
        .iter()
        .filter_map(|item| item.proposal_id.clone())
        .collect::<Vec<_>>();
    let any_memory_updated = results.iter().any(|item| item.memory_updated);
    let any_concept_updated = results.iter().any(|item| item.concept_graph_updated);
    let status = decide_trigger_status_from_module_results(&results);

    let first_error = results.iter().find(|item| item.error_code.is_some());
    let error_code = first_error.and_then(|item| item.error_code);
    let error_detail = if matches!(status, "failed" | "partial") {
        let merged = results
            .iter()
            .filter_map(|item| {
                item.error_code
                    .zip(item.error_detail.as_ref())
                    .map(|(code, detail)| format!("{}:{}:{}", item.module_target, code, detail))
            })
            .collect::<Vec<_>>()
            .join(" | ");
        if merged.is_empty() {
            None
        } else {
            Some(merged)
        }
    } else {
        None
    };

    emit_trigger_processed_event(
        state,
        trigger_event_id,
        trigger_target,
        &module_targets,
        &proposal_ids,
        any_memory_updated,
        any_concept_updated,
        status,
        error_code,
        error_detail.as_deref(),
    )
    .await;
}

async fn run_module_worker(
    state: &AppState,
    trigger_event_id: &str,
    module_target: &str,
    reason: &str,
) -> ModuleProcessResult {
    let recent_event_history =
        format_event_history(state, state.limits.submodule_history, None, None).await;
    let input = json!({
        "trigger_event_id": trigger_event_id,
        "module_target": module_target,
        "reason": reason,
        "recent_event_history": recent_event_history,
    })
    .to_string();
    let self_improvement_instructions = state
        .prompts
        .read()
        .await
        .self_improvement
        .clone()
        .unwrap_or_default();
    if self_improvement_instructions.trim().is_empty() {
        return ModuleProcessResult {
            module_target: module_target.to_string(),
            status: "failed",
            memory_updated: false,
            concept_graph_updated: false,
            concept_ensured: None,
            proposal_id: None,
            error_code: Some("SELF_IMPROVEMENT_INSTRUCTIONS_MISSING"),
            error_detail: Some("prompts.md missing non-empty `# Self Improvement`".to_string()),
        };
    }
    let adapter = ResponseApiAdapter::new(ResponseApiConfig {
        model: state.modules.runtime.model.clone(),
        instructions: self_improvement_instructions,
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
            return ModuleProcessResult {
                module_target: module_target.to_string(),
                status: "failed",
                memory_updated: false,
                concept_graph_updated: false,
                concept_ensured: None,
                proposal_id: None,
                error_code: Some("TRIGGER_LLM_CALL_FAILED"),
                error_detail: Some(err.to_string()),
            };
        }
    };

    emit_trigger_debug_raw(state, trigger_event_id, module_target, &input, &response).await;

    let plan = match parse_trigger_processing_plan(response.text.as_str()) {
        Ok(value) => value,
        Err(err) => {
            return ModuleProcessResult {
                module_target: module_target.to_string(),
                status: "failed",
                memory_updated: false,
                concept_graph_updated: false,
                concept_ensured: None,
                proposal_id: None,
                error_code: Some("TRIGGER_PLAN_PARSE_FAILED"),
                error_detail: Some(err),
            };
        }
    };

    let mut memory_updated = false;
    let mut concept_graph_updated = false;
    let mut concept_ensured = None::<bool>;
    let mut proposal_id = None::<String>;
    let mut issues = Vec::<TriggerWorkerIssue>::new();

    if let Some(name) = submodule_name_from_target(module_target) {
        let concept_name = format!("submodule:{}", name);
        match state
            .activation_concept_graph
            .concept_upsert(concept_name)
            .await
        {
            Ok(value) => {
                let created = value
                    .get("created")
                    .and_then(|item| item.as_bool())
                    .unwrap_or(false);
                concept_ensured = Some(true);
                if created {
                    concept_graph_updated = true;
                }
            }
            Err(err) => {
                return ModuleProcessResult {
                    module_target: module_target.to_string(),
                    status: "failed",
                    memory_updated: false,
                    concept_graph_updated: false,
                    concept_ensured: Some(false),
                    proposal_id: None,
                    error_code: Some("SUBMODULE_CONCEPT_ENSURE_FAILED"),
                    error_detail: Some(err),
                };
            }
        }
    }

    if let Some(memory_plan) = plan.memory_section_update {
        match apply_memory_section_update(state, module_target, &memory_plan).await {
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
            .unwrap_or_else(|| module_target.to_string());
        match propose_improvement(
            state,
            DebugImproveProposalRequest {
                target: proposal_target,
                job_id: format!("trigger:{}:{}", trigger_event_id, module_target),
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

    ModuleProcessResult {
        module_target: module_target.to_string(),
        status,
        memory_updated,
        concept_graph_updated,
        concept_ensured,
        proposal_id,
        error_code,
        error_detail,
    }
}

fn trigger_worker_semaphore() -> Arc<Semaphore> {
    static TRIGGER_WORKER_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    TRIGGER_WORKER_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(TRIGGER_WORKER_MAX_CONCURRENCY)))
        .clone()
}

async fn resolve_trigger_targets(
    state: &AppState,
    trigger_target: &str,
) -> Result<Vec<String>, String> {
    let target = trigger_target.trim();
    if target.is_empty() || target.eq_ignore_ascii_case("all") {
        return resolve_all_targets(state).await;
    }

    if target.eq_ignore_ascii_case("submodules") {
        let modules = state
            .modules
            .registry
            .list_active()
            .await
            .map_err(|err| err.to_string())?;
        return Ok(modules
            .into_iter()
            .map(|module| format!("submodule:{}", module.name))
            .collect::<Vec<_>>());
    }

    if PromptTarget::parse(target).is_some() {
        return Ok(vec![target.to_string()]);
    }

    Err(format!("invalid trigger target: {}", target))
}

async fn resolve_all_targets(state: &AppState) -> Result<Vec<String>, String> {
    let modules = state
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| err.to_string())?;
    let mut targets = vec![
        "base".to_string(),
        "router".to_string(),
        "decision".to_string(),
    ];
    for module in modules {
        targets.push(format!("submodule:{}", module.name));
    }
    Ok(targets)
}

fn decide_trigger_status_from_module_results(results: &[ModuleProcessResult]) -> &'static str {
    if results.is_empty() {
        return "failed";
    }
    let all_success = results.iter().all(|item| item.status == "success");
    if all_success {
        return "success";
    }
    let all_failed = results.iter().all(|item| item.status == "failed");
    if all_failed {
        return "failed";
    }
    "partial"
}

fn submodule_name_from_target(target: &str) -> Option<&str> {
    let value = target.trim();
    let prefix = "submodule:";
    let body = value.strip_prefix(prefix)?;
    let name = body.trim();
    if name.is_empty() {
        return None;
    }
    Some(name)
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

fn payload_str_or(value: Option<&Value>, fallback: &str) -> String {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

async fn emit_trigger_debug_raw(
    state: &AppState,
    trigger_event_id: &str,
    module_target: &str,
    input: &str,
    response: &crate::llm::LlmResponse,
) {
    let event = build_event(
        TRIGGER_WORKER_SOURCE,
        "text",
        json!({
            "trigger_event_id": trigger_event_id,
            "module_target": module_target,
            "input": input,
            "output_text": response.text,
            "raw": response.raw,
            "tool_calls": response.tool_calls,
        }),
        vec![
            "debug".to_string(),
            "llm.raw".to_string(),
            "module:self_improvement".to_string(),
        ],
    );
    crate::record_event(state, event).await;
}

async fn emit_module_processed_event(
    state: &AppState,
    trigger_event_id: &str,
    result: &ModuleProcessResult,
) {
    let mut payload = json!({
        "trigger_event_id": trigger_event_id,
        "module_target": result.module_target,
        "status": result.status,
        "memory_updated": result.memory_updated,
        "concept_graph_updated": result.concept_graph_updated,
        "processed_at": now_iso8601(),
    });
    if let Some(value) = result.concept_ensured {
        payload["concept_ensured"] = json!(value);
    }
    if let Some(value) = &result.proposal_id {
        payload["proposal_id"] = json!(value);
    }
    if let Some(value) = result.error_code {
        payload["error_code"] = json!(value);
    }
    if let Some(value) = &result.error_detail {
        payload["error_detail"] = json!(value);
    }
    let event = build_event(
        TRIGGER_WORKER_SOURCE,
        "text",
        payload,
        vec!["self_improvement.module_processed".to_string()],
    );
    crate::record_event(state, event).await;
}

async fn emit_trigger_processed_event(
    state: &AppState,
    trigger_event_id: &str,
    trigger_target: &str,
    resolved_targets: &[String],
    proposal_ids: &[String],
    memory_updated: bool,
    concept_graph_updated: bool,
    status: &str,
    error_code: Option<&str>,
    error_detail: Option<&str>,
) {
    let mut payload = json!({
        "trigger_event_id": trigger_event_id,
        "target": trigger_target,
        "resolved_targets": resolved_targets,
        "proposal_ids": proposal_ids,
        "status": status,
        "memory_updated": memory_updated,
        "concept_graph_updated": concept_graph_updated,
        "processed_at": now_iso8601(),
    });
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
        vec![
            "self_improvement.trigger_processed".to_string(),
            "debug".to_string(),
        ],
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
    let target = match PromptTarget::parse(target_raw) {
        Some(value) => value,
        None => {
            return Err(format!(
                "invalid memory_section_update target: {}",
                target_raw
            ));
        }
    };
    if !matches!(target, PromptTarget::Decision) {
        return Err(format!(
            "memory_section_update target '{}' is not allowed; only 'decision' is allowed",
            target_raw
        ));
    }

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
        decide_trigger_processed_status, parse_trigger_processing_plan, submodule_name_from_target,
    };

    fn normalize_trigger_target_for_prompt(target: &str) -> String {
        let trimmed = target.trim();
        if trimmed.eq_ignore_ascii_case("base")
            || trimmed.eq_ignore_ascii_case("router")
            || trimmed.eq_ignore_ascii_case("decision")
            || trimmed.to_lowercase().starts_with("submodule:")
        {
            return trimmed.to_string();
        }
        "base".to_string()
    }

    #[test]
    fn parse_trigger_plan_accepts_plain_json() {
        let raw = r#"{
            "memory_section_update": {"target":"decision","content":"updated"},
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

    #[test]
    fn submodule_name_from_target_parses_expected_format() {
        assert_eq!(
            submodule_name_from_target("submodule:curiosity"),
            Some("curiosity")
        );
        assert_eq!(
            submodule_name_from_target("submodule:  self_preservation "),
            Some("self_preservation")
        );
        assert_eq!(submodule_name_from_target("router"), None);
        assert_eq!(submodule_name_from_target("submodule:"), None);
    }
}
