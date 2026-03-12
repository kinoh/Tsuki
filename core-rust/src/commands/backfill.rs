use crate::activation_concept_graph::ActivationConceptGraphStore;

pub(crate) async fn run(limit: Option<usize>) -> Result<(), String> {
    let arousal_tau_ms = std::env::var("AROUSAL_TAU_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(86_400_000.0);
    let store = ActivationConceptGraphStore::connect(
        std::env::var("MEMGRAPH_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string()),
        std::env::var("MEMGRAPH_USER").unwrap_or_default(),
        std::env::var("MEMGRAPH_PASSWORD").unwrap_or_default(),
        arousal_tau_ms,
        None,
    )
    .await?;

    let (updated, failed) = store.backfill_concept_embeddings(limit).await?;
    println!(
        "EMBED_BACKFILL_RESULT updated={} failed={} limit={}",
        updated,
        failed,
        limit
            .map(|value| value.to_string())
            .unwrap_or_else(|| "all".to_string())
    );
    if failed > 0 {
        return Err(format!("backfill failed for {} concepts", failed));
    }
    Ok(())
}
