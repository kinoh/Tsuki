use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app_state::AppState;

const DEFAULT_LIST_LIMIT: usize = 100;
const MAX_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StateRecordListItem {
    pub(crate) key: String,
    pub(crate) content_preview: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StateRecordDetail {
    pub(crate) key: String,
    pub(crate) content: String,
    pub(crate) related_keys: Vec<String>,
    pub(crate) metadata: Value,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct StateRecordUpsertPayload {
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) related_keys: Vec<String>,
    #[serde(default)]
    pub(crate) metadata: Value,
}

pub(crate) async fn list_state_records(
    state: &AppState,
    query: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<StateRecordListItem>, (StatusCode, String)> {
    let limit = limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .max(1)
        .min(MAX_LIST_LIMIT);
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
    let items = rows
        .into_iter()
        .map(
            |(key, content, _related_keys_json, _metadata_json, updated_at)| StateRecordListItem {
                key,
                content_preview: truncate(content.as_str(), 160),
                updated_at,
            },
        )
        .collect();
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
    Ok(Some(StateRecordDetail {
        key: key.to_string(),
        content,
        related_keys,
        metadata,
        updated_at,
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

    state
        .services
        .db
        .upsert_state_record(
            trimmed_key,
            payload.content.as_str(),
            serde_json::to_string(&payload.related_keys)
                .unwrap_or_else(|_| "[]".to_string())
                .as_str(),
            serde_json::to_string(&payload.metadata)
                .unwrap_or_else(|_| "{}".to_string())
                .as_str(),
        )
        .await
        .map_err(internal_error)?;

    get_state_record_detail(state, trimmed_key)
        .await?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "saved record disappeared".to_string(),
            )
        })
}

fn internal_error(err: impl ToString) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "..."
}
