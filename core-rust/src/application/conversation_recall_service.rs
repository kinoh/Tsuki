use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::app_state::AppState;
use crate::application::history_service::{event_role, format_local_ts_seconds, truncate};
use crate::conversation_recall_store::conversation_recall_text;
use crate::event::Event;

const RAW_LIMIT_MULTIPLIER: usize = 4;

#[derive(Debug)]
struct RankedRecallEvent {
    event: Event,
    score: f64,
}

pub(crate) async fn format_recalled_event_history(
    state: &AppState,
    input_text: &str,
    excluded_event_ids: &HashSet<String>,
) -> String {
    let config = &state.config.conversation_recall;
    if !config.enabled || config.limit == 0 {
        return "none".to_string();
    }
    let query = input_text.trim();
    if query.is_empty() {
        return "none".to_string();
    }
    let raw_limit = config
        .limit
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
    let mut ranked = Vec::<RankedRecallEvent>::new();

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
        ranked.push(RankedRecallEvent { event, score });
    }

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.event.ts.cmp(&a.event.ts))
            .then_with(|| a.event.event_id.cmp(&b.event.event_id))
    });
    ranked.truncate(config.limit);
    if ranked.is_empty() {
        return "none".to_string();
    }

    let mut lines = Vec::with_capacity(ranked.len() + 1);
    lines.push("ts | role | message | recall_score".to_string());
    for item in ranked {
        let text = conversation_recall_text(&item.event).unwrap_or_default();
        lines.push(format!(
            "{} | {} | {} | {:.3}",
            format_local_ts_seconds(item.event.ts.as_str()),
            event_role(&item.event),
            truncate(text.as_str(), 160),
            item.score,
        ));
    }
    lines.join("\n")
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
    use super::recency_score;

    #[test]
    fn recency_score_decays() {
        let now_ms = 1_741_500_800_000i64;
        let recent = recency_score("2025-03-09T00:00:00Z", now_ms, 1000.0 * 60.0 * 60.0);
        let older = recency_score("2025-03-08T00:00:00Z", now_ms, 1000.0 * 60.0 * 60.0);
        assert!(recent > older);
    }
}
