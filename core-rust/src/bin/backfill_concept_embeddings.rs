#[allow(dead_code)]
#[path = "../activation_concept_graph.rs"]
mod activation_concept_graph;

use activation_concept_graph::ActivationConceptGraphStore;
use std::env;

#[derive(Debug, Clone)]
struct Cli {
    limit: Option<usize>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("ERROR: {}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let cli = parse_cli()?;
    let arousal_tau_ms = env::var("AROUSAL_TAU_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(86_400_000.0);
    let store = ActivationConceptGraphStore::connect(
        env::var("MEMGRAPH_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string()),
        env::var("MEMGRAPH_USER").unwrap_or_default(),
        env::var("MEMGRAPH_PASSWORD").unwrap_or_default(),
        arousal_tau_ms,
    )
    .await?;

    let (updated, failed) = store.backfill_concept_embeddings(cli.limit).await?;
    println!(
        "EMBED_BACKFILL_RESULT updated={} failed={} limit={}",
        updated,
        failed,
        cli.limit
            .map(|value| value.to_string())
            .unwrap_or_else(|| "all".to_string())
    );
    if failed > 0 {
        return Err(format!("backfill failed for {} concepts", failed));
    }
    Ok(())
}

fn parse_cli() -> Result<Cli, String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        std::process::exit(0);
    }
    let mut limit = None;
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--limit" => {
                let value = args.get(idx + 1).ok_or("--limit requires a value")?;
                limit = Some(
                    value
                        .parse::<usize>()
                        .map_err(|err| format!("invalid --limit: {}", err))?
                        .max(1),
                );
                idx += 2;
            }
            option => return Err(format!("unknown option: {}", option)),
        }
    }
    Ok(Cli { limit })
}

fn print_usage() {
    println!("backfill_concept_embeddings");
    println!("Usage:");
    println!("  cargo run --bin backfill_concept_embeddings -- [--limit N]");
}
