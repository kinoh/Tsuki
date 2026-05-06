use std::collections::HashSet;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

use crate::app_state::AppState;
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
            if is_debug_event(&event) {
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

fn format_event_line(event: &Event) -> String {
    if is_observability_event(event) {
        return format!(
            "{} | observe | {}",
            format_local_ts_seconds(&event.ts),
            format_observability_event_summary(event)
        );
    }
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

fn format_observability_event_summary(event: &Event) -> String {
    let tool_name = event
        .meta
        .tags
        .iter()
        .find_map(|tag| tag.strip_prefix("tool:"))
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let status = event
        .payload
        .get("outcome")
        .and_then(|value| value.as_str())
        .or_else(|| {
            if event.meta.tags.iter().any(|tag| tag == "error") {
                Some("error")
            } else {
                None
            }
        })
        .unwrap_or("unknown");
    let args = event
        .payload
        .get("arguments")
        .map(|value| truncate(&value.to_string(), 120))
        .unwrap_or_else(|| "none".to_string());
    let output = event
        .payload
        .get("output")
        .map(|value| {
            value
                .as_str()
                .map(|text| truncate(text, 120))
                .unwrap_or_else(|| truncate(&value.to_string(), 120))
        })
        .unwrap_or_else(|| "none".to_string());
    format!(
        "tool={} status={} args={} output={}",
        tool_name, status, args, output
    )
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
    use crate::event::{rehydrate_event, Event};
    use serde_json::json;

    fn observe_event() -> Event {
        rehydrate_event(
            "observe-1".to_string(),
            "2026-03-19T14:33:45.000000000Z".to_string(),
            "tooling".to_string(),
            "state".to_string(),
            json!({
                "outcome": "ok",
                "arguments": { "command": "node", "args": ["/memory/skills/web_page_extract/scripts/fetch.js", "https://openai.com/news/"] },
                "output": { "elapsed_ms": 123, "stdout": "{ \"url\": \"https://openai.com/news/\" }" }
            }),
            vec![
                "observe".to_string(),
                "tool".to_string(),
                "tool:shell_exec__execute".to_string(),
                "outcome:ok".to_string(),
            ],
        )
    }

    #[test]
    fn format_event_lines_includes_compressed_observability_event() {
        let events = vec![
            input_text("user", "message", "hello"),
            observe_event(),
            decision_text("decision=respond reason=test".to_string(), false),
        ];
        let rendered = format_event_lines(&events);

        assert!(rendered.contains("observe | tool=shell_exec__execute status=ok"));
        assert!(rendered.contains("args="));
        assert!(rendered.contains("output="));
    }
}
