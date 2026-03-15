use std::sync::Arc;

use tokio::sync::broadcast;

use crate::app_state::AppState;
use crate::conversation_recall_store::ConversationRecallStore;
use crate::{event::Event, event_store::EventStore};

pub(crate) fn build_emit_event_callback(
    event_store: Arc<EventStore>,
    tx: broadcast::Sender<Event>,
    conversation_recall_store: Arc<dyn ConversationRecallStore>,
) -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(move |event: Event| {
        let event_store = event_store.clone();
        let tx = tx.clone();
        let conversation_recall_store = conversation_recall_store.clone();
        tokio::spawn(async move {
            if let Err(err) = event_store.append(&event).await {
                println!("EVENT_STORE_ERROR error={}", err);
            }
            let _ = tx.send(event.clone());
            log_event(&event);
            if let Err(err) = conversation_recall_store
                .upsert_event_projection(&event)
                .await
            {
                println!(
                    "CONVERSATION_RECALL_INDEX_ERROR event_id={} error={}",
                    event.event_id, err
                );
            }
        });
    })
}

pub(crate) async fn record_event(state: &AppState, event: Event) {
    if let Err(err) = state.services.event_store.append(&event).await {
        println!("EVENT_STORE_ERROR error={}", err);
    }
    let _ = state.services.tx.send(event.clone());
    log_event(&event);
    if let Err(err) = state
        .services
        .conversation_recall_store
        .upsert_event_projection(&event)
        .await
    {
        println!(
            "CONVERSATION_RECALL_INDEX_ERROR event_id={} error={}",
            event.event_id, err
        );
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "…"
}

fn log_event(event: &Event) {
    let tags = if event.meta.tags.is_empty() {
        "none".to_string()
    } else {
        event.meta.tags.join(",")
    };
    let payload_text = event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .map(|value| truncate(value, 120))
        .unwrap_or_else(|| truncate(&event.payload.to_string(), 120));
    println!(
        "EVENT ts={} source={} modality={} tags={} payload={}",
        event.ts, event.source, event.modality, tags, payload_text
    );
}
