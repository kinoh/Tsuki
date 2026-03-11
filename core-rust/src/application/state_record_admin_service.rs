use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app_state::AppState;
use crate::llm::{build_response_api_llm, LlmRequest, ResponseApiConfig};

const DEFAULT_LIST_LIMIT: usize = 100;
const MAX_LIST_LIMIT: usize = 200;
const MAX_TRIGGER_CONCEPTS: usize = 3;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StateRecordListItem {
    pub(crate) key: String,
    pub(crate) content_preview: String,
    pub(crate) updated_at: String,
    pub(crate) skill_index: SkillIndexView,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StateRecordDetail {
    pub(crate) key: String,
    pub(crate) content: String,
    pub(crate) related_keys: Vec<String>,
    pub(crate) metadata: Value,
    pub(crate) updated_at: String,
    pub(crate) skill_index: SkillIndexView,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillIndexView {
    pub(crate) enabled: bool,
    pub(crate) summary: Option<String>,
    pub(crate) body_state_key: Option<String>,
    pub(crate) trigger_concepts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct StateRecordUpsertPayload {
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) related_keys: Vec<String>,
    #[serde(default)]
    pub(crate) metadata: Value,
    #[serde(default)]
    pub(crate) skill_index: Option<SkillIndexUpsertPayload>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SkillIndexUpsertPayload {
    pub(crate) enabled: bool,
}

#[derive(Debug, Deserialize)]
struct GeneratedSkillIndex {
    summary: String,
    #[serde(default)]
    trigger_concepts: Vec<String>,
}

pub(crate) async fn list_state_records(
    state: &AppState,
    query: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<StateRecordListItem>, (StatusCode, String)> {
    let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT).max(1).min(MAX_LIST_LIMIT);
    let rows = match query.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => state
            .services
            .db
            .search_state_records(value, limit)
            .await
            .map_err(internal_error)?,
        None => state
            .services
            .db
            .list_state_records(limit)
            .await
            .map_err(internal_error)?,
    };
    let mut items = Vec::with_capacity(rows.len());
    for (key, content, _related_keys_json, _metadata_json, updated_at) in rows {
        let skill_index = load_skill_index(state, key.as_str()).await?;
        items.push(StateRecordListItem {
            key,
            content_preview: truncate(content.as_str(), 160),
            updated_at,
            skill_index,
        });
    }
    Ok(items)
}

pub(crate) async fn get_state_record_detail(
    state: &AppState,
    key: &str,
) -> Result<Option<StateRecordDetail>, (StatusCode, String)> {
    let row = state
        .services
        .db
        .get_state_record(key)
        .await
        .map_err(internal_error)?;
    let Some((content, related_keys_json, metadata_json, updated_at)) = row else {
        return Ok(None);
    };
    let related_keys = serde_json::from_str::<Vec<String>>(&related_keys_json).unwrap_or_default();
    let metadata = serde_json::from_str::<Value>(&metadata_json).unwrap_or(Value::Null);
    let skill_index = load_skill_index(state, key).await?;
    Ok(Some(StateRecordDetail {
        key: key.to_string(),
        content,
        related_keys,
        metadata,
        updated_at,
        skill_index,
    }))
}

pub(crate) async fn upsert_state_record(
    state: &AppState,
    key: &str,
    payload: StateRecordUpsertPayload,
) -> Result<StateRecordDetail, (StatusCode, String)> {
    let trimmed_key = key.trim();
    if trimmed_key.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "key is required".to_string()));
    }
    if payload.content.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "content is required".to_string()));
    }

    state.services.db.upsert_state_record(
        trimmed_key,
        payload.content.as_str(),
        serde_json::to_string(&payload.related_keys)
            .unwrap_or_else(|_| "[]".to_string())
            .as_str(),
        serde_json::to_string(&payload.metadata)
            .unwrap_or_else(|_| "{}".to_string())
            .as_str(),
    ).await.map_err(internal_error)?;

    let skill_name = skill_name_for_key(trimmed_key);
    let existing_skill_index = load_skill_index(state, trimmed_key).await?;
    let skill_index_enabled = payload.skill_index.as_ref().map(|item| item.enabled).unwrap_or(false);
    if skill_index_enabled {
        let generated = generate_skill_index(state, trimmed_key, payload.content.as_str()).await?;
        state
            .services
            .activation_concept_graph
            .skill_index_upsert(
                skill_name.clone(),
                generated.summary.clone(),
                trimmed_key.to_string(),
                true,
            )
            .await
            .map_err(internal_error)?;
        state
            .services
            .activation_concept_graph
            .skill_index_replace_triggers(skill_name, generated.trigger_concepts)
            .await
            .map_err(internal_error)?;
    } else if existing_skill_index.enabled {
        state
            .services
            .activation_concept_graph
            .skill_index_upsert(
                skill_name,
                existing_skill_index.summary.unwrap_or_else(|| trimmed_key.to_string()),
                trimmed_key.to_string(),
                false,
            )
            .await
            .map_err(internal_error)?;
    }

    get_state_record_detail(state, trimmed_key)
        .await?
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "saved record disappeared".to_string()))
}

fn internal_error(err: impl ToString) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn skill_name_for_key(key: &str) -> String {
    format!("skill:{}", key.trim())
}

async fn load_skill_index(
    state: &AppState,
    key: &str,
) -> Result<SkillIndexView, (StatusCode, String)> {
    let name = skill_name_for_key(key);
    let detail = state
        .services
        .activation_concept_graph
        .debug_concept_detail(name)
        .await
        .map_err(internal_error)?;
    let Some(detail) = detail else {
        return Ok(SkillIndexView {
            enabled: false,
            summary: None,
            body_state_key: None,
            trigger_concepts: Vec::new(),
        });
    };
    let summary = detail
        .get("summary")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty());
    let body_state_key = detail
        .get("body_state_key")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty());
    let disabled = detail
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let trigger_concepts = detail
        .get("relations")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("direction").and_then(Value::as_str) == Some("incoming"))
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("evokes"))
                .filter_map(|item| item.get("from").and_then(Value::as_str))
                .filter(|value| !value.starts_with("skill:") && !value.starts_with("submodule:"))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(SkillIndexView {
        enabled: !disabled,
        summary,
        body_state_key,
        trigger_concepts,
    })
}

async fn generate_skill_index(
    state: &AppState,
    key: &str,
    body: &str,
) -> Result<GeneratedSkillIndex, (StatusCode, String)> {
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
state_key: {key}\n\
body:\n{body}",
        max_trigger_concepts = MAX_TRIGGER_CONCEPTS,
        key = key,
        body = body
    );
    let response = adapter
        .respond(LlmRequest { input: prompt })
        .await
        .map_err(|err| internal_error(format!("skill index generation failed: {}", err)))?;
    parse_generated_skill_index(response.text.as_str())
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))
}

fn parse_generated_skill_index(raw: &str) -> Result<GeneratedSkillIndex, String> {
    let text = raw.trim();
    let parsed = serde_json::from_str::<GeneratedSkillIndex>(text)
        .or_else(|_| extract_first_json_object(text).and_then(|json| serde_json::from_str::<GeneratedSkillIndex>(json.as_str()).map_err(|err| err.to_string())))
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

fn extract_first_json_object(raw: &str) -> Result<String, String> {
    let start = raw.find('{').ok_or_else(|| "missing json object start".to_string())?;
    let end = raw.rfind('}').ok_or_else(|| "missing json object end".to_string())?;
    if end < start {
        return Err("invalid json object range".to_string());
    }
    Ok(raw[start..=end].to_string())
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_generated_skill_index_accepts_wrapped_json() {
        let raw = "noise\n{\"summary\":\"Abstract conversational guidance\",\"trigger_concepts\":[\"gentle talk\",\"comfort\"]}\nnoise";
        let parsed = parse_generated_skill_index(raw).expect("should parse");
        assert_eq!(parsed.summary, "Abstract conversational guidance");
        assert_eq!(parsed.trigger_concepts, vec!["gentle talk".to_string(), "comfort".to_string()]);
    }
}
