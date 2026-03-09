use serde_json::json;
use std::collections::{HashMap, HashSet};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

use crate::app_state::AppState;
use crate::event::contracts::role_text_output;
use crate::event::Event;

pub(crate) async fn format_event_history(
    state: &AppState,
    limit: usize,
    cutoff_ts: Option<&str>,
    excluded_event_ids: Option<&HashSet<String>>,
) -> String {
    let events = latest_events(state, limit, cutoff_ts, excluded_event_ids).await;
    format_event_lines(&events)
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
    format_event_lines(&events)
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
    let batch_size = limit.saturating_mul(4).clamp(50, 500);
    let max_scanned = 5_000usize;
    let mut visible = Vec::<Event>::with_capacity(limit);
    let mut scanned = 0usize;
    let mut cursor: Option<(String, String)> = None;

    while visible.len() < limit && scanned < max_scanned {
        let batch = match &cursor {
            Some((ts, event_id)) => {
                state
                    .services
                    .event_store
                    .list_before_anchor(ts.as_str(), event_id.as_str(), batch_size)
                    .await
            }
            None => {
                state
                    .services
                    .event_store
                    .list(batch_size, None, true)
                    .await
            }
        };
        let batch = match batch {
            Ok(events) => events,
            Err(err) => {
                println!("EVENT_STORE_ERROR error={}", err);
                return Vec::new();
            }
        };
        if batch.is_empty() {
            break;
        }
        scanned += batch.len();
        cursor = batch
            .last()
            .map(|event| (event.ts.clone(), event.event_id.clone()));

        for event in batch {
            if is_debug_event(&event) || is_observability_event(&event) {
                continue;
            }
            if excluded_event_ids
                .map(|ids| ids.contains(event.event_id.as_str()))
                .unwrap_or(false)
            {
                continue;
            }
            if cutoff_ts
                .map(|cutoff| event.ts.as_str() < cutoff)
                .unwrap_or(false)
            {
                continue;
            }
            visible.push(event);
            if visible.len() >= limit {
                break;
            }
        }
    }
    visible.reverse();
    visible
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
            role_text_output(
                format!("submodule:{}", name).as_str(),
                "submodule",
                text.to_string(),
                false,
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

pub(crate) fn format_event_lines(events: &[Event]) -> String {
    if events.is_empty() {
        return "none".to_string();
    }
    let mut lines = Vec::with_capacity(events.len() + 1);
    lines.push("ts | role | message".to_string());
    lines.extend(events.iter().map(format_event_line));
    lines.join("\n")
}

pub(crate) fn event_role(event: &Event) -> String {
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

pub(crate) fn format_local_ts_seconds(ts: &str) -> String {
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

pub(crate) fn truncate(value: &str, max: usize) -> String {
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
    use crate::event::contracts::{decision_text, input_text};

    #[test]
    fn apply_submodule_output_overrides_replaces_and_inserts() {
        let mut events = vec![
            input_text("user", "message", "hello"),
            role_text_output(
                "submodule:curiosity",
                "submodule",
                "old curiosity".to_string(),
                false,
            ),
            decision_text("decision=respond reason=test".to_string(), false),
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
