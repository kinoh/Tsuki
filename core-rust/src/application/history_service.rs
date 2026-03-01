use serde_json::json;
use std::collections::{HashMap, HashSet};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

use crate::event::{build_event, Event};
use crate::AppState;

pub(crate) async fn format_event_history(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
) -> String {
    let events = latest_events(state, limit, cutoff_ts, excluded_event_ids).await;
    if events.is_empty() {
        return "none".to_string();
    }
    let mut lines = Vec::with_capacity(events.len() + 1);
    lines.push("ts | role | message".to_string());
    lines.extend(events.iter().map(format_event_line));
    lines.join("\n")
}

pub(crate) async fn format_decision_debug_history(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
    submodule_outputs_raw: Option<&str>,
) -> String {
    let mut events = latest_events(state, limit, cutoff_ts, excluded_event_ids).await;
    let submodule_overrides = parse_submodule_outputs(submodule_outputs_raw)
        .into_iter()
        .collect::<HashMap<_, _>>();
    if !submodule_overrides.is_empty() {
        apply_submodule_output_overrides(&mut events, &submodule_overrides);
    }
    if events.is_empty() {
        return "none".to_string();
    }
    let mut lines = Vec::with_capacity(events.len() + 1);
    lines.push("ts | role | message".to_string());
    lines.extend(events.iter().map(format_event_line));
    lines.join("\n")
}

pub(crate) async fn latest_events(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
) -> Vec<Event> {
    if limit == 0 {
        return Vec::new();
    }
    match state.event_store.latest(limit).await {
        Ok(events) => events
            .into_iter()
            .filter(|event| !is_debug_event(event))
            .filter(|event| !is_observability_event(event))
            .filter(|event| {
                excluded_event_ids
                    .map(|ids| !ids.contains(event.event_id.as_str()))
                    .unwrap_or(true)
            })
            .filter(|event| {
                cutoff_ts
                    .map(|cutoff| event.ts.as_str() >= cutoff)
                    .unwrap_or(true)
            })
            .collect(),
        Err(err) => {
            println!("EVENT_STORE_ERROR error={}", err);
            Vec::new()
        }
    }
}

pub(crate) fn is_user_input_event(event: &Event) -> bool {
    event.source == "user" && event.meta.tags.iter().any(|tag| tag == "input")
}

pub(crate) fn is_decision_event(event: &Event) -> bool {
    event.meta.tags.iter().any(|tag| tag == "decision")
}

fn parse_submodule_outputs(raw: Option<&str>) -> Vec<(String, String)> {
    let raw = match raw {
        Some(value) => value,
        None => return Vec::new(),
    };
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (name, value) = line.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                return None;
            }
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

fn apply_submodule_output_overrides(events: &mut Vec<Event>, overrides: &HashMap<String, String>) {
    let mut applied = HashSet::<String>::new();
    for event in events.iter_mut() {
        let Some(module_name) = event_submodule_name(event).map(str::to_string) else {
            continue;
        };
        let Some(override_text) = overrides.get(&module_name) else {
            continue;
        };
        event.payload = json!({ "text": override_text });
        applied.insert(module_name);
    }
    let missing = overrides
        .iter()
        .filter(|(name, _)| !applied.contains(name.as_str()))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }
    let insert_index = events
        .iter()
        .rposition(is_user_input_event)
        .map(|index| index + 1)
        .unwrap_or(events.len());
    let mut synthetic = missing
        .into_iter()
        .map(|(name, text)| {
            build_event(
                format!("submodule:{}", name).as_str(),
                "text",
                json!({ "text": text }),
                vec!["submodule".to_string()],
            )
        })
        .collect::<Vec<_>>();
    events.splice(insert_index..insert_index, synthetic.drain(..));
}

fn event_submodule_name(event: &Event) -> Option<&str> {
    if let Some(name) = event
        .source
        .strip_prefix("submodule:")
        .filter(|value| !value.is_empty())
    {
        return Some(name);
    }
    if !event.meta.tags.iter().any(|tag| tag == "submodule") {
        return None;
    }
    event
        .meta
        .tags
        .iter()
        .find_map(|tag| tag.strip_prefix("module:"))
        .filter(|value| !value.is_empty())
}

fn format_event_line(event: &Event) -> String {
    let role = event_role(event);
    let ts = format_local_ts_seconds(&event.ts);
    let payload_text = event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .map(|value| truncate(value, 160))
        .unwrap_or_else(|| truncate(&event.payload.to_string(), 160));
    format!("{} | {} | {}", ts, role, payload_text)
}

fn event_role(event: &Event) -> String {
    let tags = &event.meta.tags;
    if event.source == "user" {
        return "user".to_string();
    }
    if tags.iter().any(|tag| tag == "response") {
        return "assistant".to_string();
    }
    if tags.iter().any(|tag| tag == "decision") {
        return "decision".to_string();
    }
    if let Some(module_name) = event
        .source
        .strip_prefix("submodule:")
        .filter(|value| !value.is_empty())
    {
        return format!("submodule:{}", module_name);
    }
    if tags.iter().any(|tag| tag == "submodule") {
        if let Some(module_name) = tags
            .iter()
            .find_map(|tag| tag.strip_prefix("module:"))
            .filter(|value| !value.is_empty())
        {
            return format!("submodule:{}", module_name);
        }
        return "submodule".to_string();
    }
    event.source.clone()
}

fn format_local_ts_seconds(ts: &str) -> String {
    let parsed = match OffsetDateTime::parse(ts, &Rfc3339) {
        Ok(value) => value,
        Err(_) => return ts.to_string(),
    };
    let local = match UtcOffset::current_local_offset() {
        Ok(offset) => parsed.to_offset(offset),
        Err(_) => parsed,
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.year(),
        local.month() as u8,
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "…"
}

fn is_debug_event(event: &Event) -> bool {
    event.meta.tags.iter().any(|tag| tag == "debug")
}

fn is_observability_event(event: &Event) -> bool {
    event.meta.tags.iter().any(|tag| tag == "observe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_submodule_output_overrides_replaces_and_inserts() {
        let mut events = vec![
            build_event(
                "user",
                "text",
                json!({ "text": "hello" }),
                vec!["input".to_string(), "type:message".to_string()],
            ),
            build_event(
                "submodule:curiosity",
                "text",
                json!({ "text": "old curiosity" }),
                vec!["submodule".to_string()],
            ),
            build_event(
                "decision",
                "text",
                json!({ "text": "decision=respond reason=test" }),
                vec!["decision".to_string()],
            ),
        ];
        let overrides = HashMap::from([
            ("curiosity".to_string(), "new curiosity".to_string()),
            ("social_approval".to_string(), "new social".to_string()),
        ]);
        apply_submodule_output_overrides(&mut events, &overrides);

        let curiosity = events
            .iter()
            .find(|event| event.source == "submodule:curiosity")
            .expect("curiosity event should exist");
        assert_eq!(
            curiosity
                .payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            "new curiosity"
        );

        let inserted = events
            .iter()
            .find(|event| event.source == "submodule:social_approval")
            .expect("social_approval event should be inserted");
        assert_eq!(
            inserted
                .payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            "new social"
        );
    }
}
