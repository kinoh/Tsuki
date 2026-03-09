use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::app_state::AppState;
use crate::application::history_service::{event_role, format_local_ts_seconds, truncate};
use crate::conversation_recall_store::conversation_recall_text;
use crate::event::Event;

const RAW_LIMIT_MULTIPLIER: usize = 4;
const WINDOW_FETCH_MULTIPLIER: usize = 4;
const WINDOW_FETCH_MAX_BATCH: usize = 200;

#[derive(Debug)]
struct RankedRecallAnchor {
    event: Event,
    score: f64,
}

#[derive(Debug)]
struct RecalledConversationEvent {
    event: Event,
    anchor_score: f64,
}

pub(crate) async fn format_recalled_event_history(
    state: &AppState,
    input_text: &str,
    excluded_event_ids: &HashSet<String>,
) -> String {
    let config = &state.config.conversation_recall;
    if !config.enabled || config.top_k_hits == 0 {
        return "none".to_string();
    }
    let query = input_text.trim();
    if query.is_empty() {
        return "none".to_string();
    }
    let raw_limit = config
        .top_k_hits
        .max(1)
        .saturating_mul(RAW_LIMIT_MULTIPLIER.max(1));
    let candidates = match state
        .services
        .conversation_recall_store
        .search_event_projections(query, raw_limit)
        .await
    {
        Ok(items) => items,
        Err(err) => {
            println!(
                "CONVERSATION_RECALL_SEARCH_ERROR query_len={} error={}",
                query.len(),
                err
            );
            return "none".to_string();
        }
    };
    if candidates.is_empty() {
        return "none".to_string();
    }

    let semantic_weight = config.semantic_weight.clamp(0.0, 1.0);
    let recency_weight = config.recency_weight.clamp(0.0, 1.0);
    let recency_tau_ms = config.recency_tau_ms.max(1.0);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0);
    let mut ranked = Vec::<RankedRecallAnchor>::new();

    for candidate in candidates {
        if excluded_event_ids.contains(candidate.event_id.as_str()) {
            continue;
        }
        let event = match state
            .services
            .event_store
            .get_by_id(candidate.event_id.as_str())
            .await
        {
            Ok(Some(event)) => event,
            Ok(None) => continue,
            Err(err) => {
                println!(
                    "CONVERSATION_RECALL_EVENT_LOAD_ERROR event_id={} error={}",
                    candidate.event_id, err
                );
                continue;
            }
        };
        if conversation_recall_text(&event).is_none() {
            continue;
        }
        let recency = recency_score(event.ts.as_str(), now_ms, recency_tau_ms);
        let score = round_score(
            (candidate.semantic_similarity.clamp(0.0, 1.0) * semantic_weight)
                + (recency * recency_weight),
        );
        ranked.push(RankedRecallAnchor { event, score });
    }

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.event.ts.cmp(&a.event.ts))
            .then_with(|| a.event.event_id.cmp(&b.event.event_id))
    });
    ranked.truncate(config.top_k_hits);
    if ranked.is_empty() {
        return "none".to_string();
    }

    let expanded = expand_recall_windows(
        state,
        &ranked,
        config.surrounding_event_window,
        excluded_event_ids,
    )
    .await;
    if expanded.is_empty() {
        return "none".to_string();
    }

    let mut lines = Vec::with_capacity(expanded.len() + 1);
    lines.push("ts | role | message | recall_score".to_string());
    for item in expanded {
        let text = conversation_recall_text(&item.event).unwrap_or_default();
        lines.push(format!(
            "{} | {} | {} | {:.3}",
            format_local_ts_seconds(item.event.ts.as_str()),
            event_role(&item.event),
            truncate(text.as_str(), 160),
            item.anchor_score,
        ));
    }
    lines.join("\n")
}

async fn expand_recall_windows(
    state: &AppState,
    anchors: &[RankedRecallAnchor],
    surrounding_event_window: usize,
    excluded_event_ids: &HashSet<String>,
) -> Vec<RecalledConversationEvent> {
    let mut merged = HashMap::<String, RecalledConversationEvent>::new();
    for anchor in anchors {
        remember_recalled_event(&mut merged, &anchor.event, anchor.score, excluded_event_ids);
        if surrounding_event_window == 0 {
            continue;
        }
        let before_events = load_neighbor_conversation_events(
            state,
            &anchor.event,
            NeighborDirection::Before,
            surrounding_event_window,
            excluded_event_ids,
        )
        .await;
        for event in before_events {
            remember_recalled_event(&mut merged, &event, anchor.score, excluded_event_ids);
        }
        let after_events = load_neighbor_conversation_events(
            state,
            &anchor.event,
            NeighborDirection::After,
            surrounding_event_window,
            excluded_event_ids,
        )
        .await;
        for event in after_events {
            remember_recalled_event(&mut merged, &event, anchor.score, excluded_event_ids);
        }
    }

    let mut items = merged.into_values().collect::<Vec<_>>();
    items.sort_by(|a, b| {
        a.event
            .ts
            .cmp(&b.event.ts)
            .then_with(|| a.event.event_id.cmp(&b.event.event_id))
    });
    items
}

fn remember_recalled_event(
    merged: &mut HashMap<String, RecalledConversationEvent>,
    event: &Event,
    anchor_score: f64,
    excluded_event_ids: &HashSet<String>,
) {
    if excluded_event_ids.contains(event.event_id.as_str())
        || conversation_recall_text(event).is_none()
    {
        return;
    }
    let entry = merged
        .entry(event.event_id.clone())
        .or_insert_with(|| RecalledConversationEvent {
            event: event.clone(),
            anchor_score,
        });
    if anchor_score > entry.anchor_score {
        entry.anchor_score = anchor_score;
    }
}

#[derive(Debug, Clone, Copy)]
enum NeighborDirection {
    Before,
    After,
}

async fn load_neighbor_conversation_events(
    state: &AppState,
    anchor: &Event,
    direction: NeighborDirection,
    limit: usize,
    excluded_event_ids: &HashSet<String>,
) -> Vec<Event> {
    if limit == 0 {
        return Vec::new();
    }
    let batch_size = limit
        .saturating_mul(WINDOW_FETCH_MULTIPLIER)
        .max(limit)
        .min(WINDOW_FETCH_MAX_BATCH.max(limit));
    let mut matched = Vec::<Event>::new();
    let mut cursor_ts = anchor.ts.clone();
    let mut cursor_event_id = anchor.event_id.clone();

    loop {
        if matched.len() >= limit {
            break;
        }
        let batch = match direction {
            NeighborDirection::Before => {
                state
                    .services
                    .event_store
                    .list_before_anchor(cursor_ts.as_str(), cursor_event_id.as_str(), batch_size)
                    .await
            }
            NeighborDirection::After => {
                state
                    .services
                    .event_store
                    .list_after_anchor(cursor_ts.as_str(), cursor_event_id.as_str(), batch_size)
                    .await
            }
        };
        let batch = match batch {
            Ok(items) => items,
            Err(err) => {
                println!(
                    "CONVERSATION_RECALL_WINDOW_LOAD_ERROR anchor_event_id={} direction={} error={}",
                    anchor.event_id,
                    direction_name(direction),
                    err
                );
                break;
            }
        };
        if batch.is_empty() {
            break;
        }
        for event in &batch {
            if excluded_event_ids.contains(event.event_id.as_str()) {
                continue;
            }
            if conversation_recall_text(event).is_none() {
                continue;
            }
            matched.push(event.clone());
            if matched.len() >= limit {
                break;
            }
        }
        if let Some(last) = batch.last() {
            cursor_ts = last.ts.clone();
            cursor_event_id = last.event_id.clone();
        } else {
            break;
        }
    }

    if matches!(direction, NeighborDirection::Before) {
        matched.reverse();
    }
    matched
}

fn direction_name(direction: NeighborDirection) -> &'static str {
    match direction {
        NeighborDirection::Before => "before",
        NeighborDirection::After => "after",
    }
}

fn recency_score(ts: &str, now_ms: i64, tau_ms: f64) -> f64 {
    let Ok(parsed) = OffsetDateTime::parse(ts, &Rfc3339) else {
        return 0.0;
    };
    let event_ms = parsed.unix_timestamp_nanos() / 1_000_000;
    let age_ms = (now_ms as i128 - event_ms).max(0) as f64;
    (-age_ms / tau_ms).exp().clamp(0.0, 1.0)
}

fn round_score(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::{direction_name, recency_score, round_score, NeighborDirection};

    #[test]
    fn recency_score_decays() {
        let now_ms = 1_741_500_800_000i64;
        let recent = recency_score("2025-03-09T00:00:00Z", now_ms, 1000.0 * 60.0 * 60.0);
        let older = recency_score("2025-03-08T00:00:00Z", now_ms, 1000.0 * 60.0 * 60.0);
        assert!(recent > older);
    }

    #[test]
    fn round_score_keeps_three_decimals() {
        assert_eq!(round_score(0.12349), 0.123);
        assert_eq!(round_score(0.1235), 0.124);
    }

    #[test]
    fn direction_name_is_stable() {
        assert_eq!(direction_name(NeighborDirection::Before), "before");
        assert_eq!(direction_name(NeighborDirection::After), "after");
    }
}
