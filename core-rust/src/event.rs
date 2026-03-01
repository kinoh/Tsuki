use crate::clock::now_iso8601;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub(crate) mod contracts;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub(crate) event_id: String,
    pub(crate) ts: String,
    pub(crate) source: String,
    pub(crate) modality: String,
    pub(crate) payload: Value,
    pub(crate) meta: EventMeta,
    #[serde(skip)]
    _sealed: (),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
    pub(crate) tags: Vec<String>,
}

fn build_event(source: &str, modality: &str, payload: Value, tags: Vec<String>) -> Event {
    Event {
        event_id: Uuid::new_v4().to_string(),
        ts: now_iso8601(),
        source: source.to_string(),
        modality: modality.to_string(),
        payload,
        meta: EventMeta { tags },
        _sealed: (),
    }
}

pub(crate) fn rehydrate_event(
    event_id: String,
    ts: String,
    source: String,
    modality: String,
    payload: Value,
    tags: Vec<String>,
) -> Event {
    Event {
        event_id,
        ts,
        source,
        modality,
        payload,
        meta: EventMeta { tags },
        _sealed: (),
    }
}
