use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::app_state::AppState;
use crate::llm::{build_response_api_llm, LlmRequest, ResponseApiConfig};

const MAX_TRIGGER_CONCEPTS: usize = 3;
const SKILL_INSTALL_TOOL: &str = "shell_exec__skill_install";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SkillUpsertPayload {
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) trigger_concepts: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillUpsertResult {
    pub(crate) key: String,
    pub(crate) summary: String,
    pub(crate) trigger_concepts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GeneratedSkillIndex {
    summary: String,
    #[serde(default)]
    trigger_concepts: Vec<String>,
}

pub(crate) async fn upsert_skill(
    state: &AppState,
    key: &str,
    payload: SkillUpsertPayload,
) -> Result<SkillUpsertResult, (StatusCode, String)> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "key is required".to_string()));
    }
    if payload.content.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "content is required".to_string()));
    }

    // 1. Install files in sandbox.
    state
        .services
        .mcp_registry
        .call_tool(
            SKILL_INSTALL_TOOL,
            json!({
                "key": key,
                "files": [{"path": "SKILL.md", "body": payload.content}]
            }),
        )
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("skill_install failed: {}", err),
            )
        })?;

    // 2. Resolve summary and trigger_concepts (explicit or LLM-generated).
    let (summary, trigger_concepts) =
        resolve_skill_index(state, &key, &payload.content, payload.summary, payload.trigger_concepts).await?;

    let skill_name = format!("skill:{}", key);

    // 3. Update concept graph.
    state
        .services
        .activation_concept_graph
        .skill_index_upsert(skill_name.clone(), summary.clone(), key.clone(), true)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    state
        .services
        .activation_concept_graph
        .skill_index_replace_triggers(skill_name, trigger_concepts.clone())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;

    Ok(SkillUpsertResult {
        key,
        summary,
        trigger_concepts,
    })
}

async fn resolve_skill_index(
    state: &AppState,
    key: &str,
    content: &str,
    summary: Option<String>,
    trigger_concepts: Option<Vec<String>>,
) -> Result<(String, Vec<String>), (StatusCode, String)> {
    match (
        summary.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        trigger_concepts.as_deref(),
    ) {
        (Some(_), None) | (None, Some(_)) => Err((
            StatusCode::BAD_REQUEST,
            "summary and trigger_concepts must be provided together".to_string(),
        )),
        (Some(s), Some(tc)) => {
            let concepts = normalize_trigger_concepts(tc)?;
            Ok((s.to_string(), concepts))
        }
        (None, None) => generate_skill_index(state, key, content).await,
    }
}

fn normalize_trigger_concepts(items: &[String]) -> Result<Vec<String>, (StatusCode, String)> {
    let mut deduped = Vec::<String>::new();
    for item in items {
        let value = item.trim();
        if value.is_empty() || deduped.iter().any(|existing| existing == value) {
            continue;
        }
        if deduped.len() >= MAX_TRIGGER_CONCEPTS {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "trigger_concepts must contain at most {} unique items",
                    MAX_TRIGGER_CONCEPTS
                ),
            ));
        }
        deduped.push(value.to_string());
    }
    if deduped.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "trigger_concepts must contain at least one non-empty item".to_string(),
        ));
    }
    Ok(deduped)
}

async fn generate_skill_index(
    state: &AppState,
    key: &str,
    body: &str,
) -> Result<(String, Vec<String>), (StatusCode, String)> {
    let adapter = build_response_api_llm(ResponseApiConfig {
        model: state.runtime.modules.runtime.model.clone(),
        instructions: "Generate skill index metadata. Return strict JSON only.".to_string(),
        temperature: state.runtime.modules.runtime.temperature,
        max_output_tokens: Some(500),
        tools: Vec::new(),
        tool_handler: None,
        usage_recorder: None,
        usage_context: None,
        max_tool_rounds: 0,
    });
    let prompt = format!(
        "Create abstract index metadata for a skill body.\n\
Return strict JSON only with this shape:\n\
{{\"summary\":\"...\",\"trigger_concepts\":[\"...\"]}}\n\
Rules:\n\
- summary must be short and abstract\n\
- summary must not encode concrete situations or step-by-step instructions\n\
- trigger_concepts must contain at most {max_trigger_concepts} concise natural-language concepts\n\
- trigger_concepts should help retrieve this skill from related user intents\n\
- no markdown\n\
- no explanation\n\
\n\
skill_key: {key}\n\
body:\n{body}",
        max_trigger_concepts = MAX_TRIGGER_CONCEPTS,
        key = key,
        body = body
    );
    let response = adapter
        .respond(LlmRequest { input: prompt })
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("skill index generation failed: {}", err),
            )
        })?;
    let parsed = parse_generated(&response.text)
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok((parsed.summary, parsed.trigger_concepts))
}

fn parse_generated(raw: &str) -> Result<GeneratedSkillIndex, String> {
    let text = raw.trim();
    let parsed = serde_json::from_str::<GeneratedSkillIndex>(text)
        .or_else(|_| {
            let start = text.find('{').ok_or_else(|| "missing json object".to_string())?;
            let end = text.rfind('}').ok_or_else(|| "missing json object end".to_string())?;
            if end < start {
                return Err("invalid json range".to_string());
            }
            serde_json::from_str::<GeneratedSkillIndex>(&text[start..=end])
                .map_err(|err| err.to_string())
        })
        .map_err(|err| format!("skill index parse failed: {}", err))?;
    let summary = parsed.summary.trim().to_string();
    if summary.is_empty() {
        return Err("skill index parse failed: empty summary".to_string());
    }
    let mut deduped = Vec::<String>::new();
    for item in parsed.trigger_concepts {
        let value = item.trim();
        if value.is_empty() || deduped.iter().any(|existing| existing == value) {
            continue;
        }
        deduped.push(value.to_string());
        if deduped.len() >= MAX_TRIGGER_CONCEPTS {
            break;
        }
    }
    Ok(GeneratedSkillIndex {
        summary,
        trigger_concepts: deduped,
    })
}
