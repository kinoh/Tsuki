use std::sync::Arc;

use crate::activation_concept_graph::ActivationConceptGraphStore;
use crate::config::load_config;
use crate::conversation_recall_store::{conversation_recall_text, ConversationRecallStore};
use crate::db::Db;
use crate::event_store::EventStore;

pub(crate) async fn run(limit: Option<usize>) -> Result<(), String> {
    let config = load_config("config.toml")?;
    let db = Db::connect(&config.db)
        .await
        .map_err(|err| format!("failed to connect db: {}", err))?;
    let event_store = EventStore::new(db.clone());
    let store = Arc::new(
        ActivationConceptGraphStore::connect(
            config.concept_graph.memgraph_uri.clone(),
            config.concept_graph.memgraph_user.clone(),
            std::env::var("MEMGRAPH_PASSWORD").unwrap_or_default(),
            config.concept_graph.arousal_tau_ms,
        )
        .await?,
    );

    let page_size = 500usize;
    let max_items = limit.unwrap_or(usize::MAX);
    let mut before_ts = None::<String>;
    let mut processed = 0usize;
    let mut indexed = 0usize;

    loop {
        if processed >= max_items {
            break;
        }
        let batch_limit = page_size.min(max_items.saturating_sub(processed));
        if batch_limit == 0 {
            break;
        }
        let events = event_store
            .list(batch_limit, before_ts.as_deref(), true)
            .await
            .map_err(|err| format!("failed to load events: {}", err))?;
        if events.is_empty() {
            break;
        }
        before_ts = events.last().map(|event| event.ts.clone());
        for event in events {
            processed += 1;
            if conversation_recall_text(&event).is_none() {
                if processed >= max_items {
                    break;
                }
                continue;
            }
            match store.upsert_event_projection(&event).await {
                Ok(_) => indexed += 1,
                Err(err) => {
                    eprintln!(
                        "CONVERSATION_RECALL_BACKFILL_ERROR event_id={} error={}",
                        event.event_id, err
                    );
                }
            }
            if processed >= max_items {
                break;
            }
        }
    }

    println!(
        "CONVERSATION_RECALL_BACKFILL_RESULT processed={} indexed={} limit={}",
        processed,
        indexed,
        limit
            .map(|value| value.to_string())
            .unwrap_or_else(|| "all".to_string())
    );
    Ok(())
}
