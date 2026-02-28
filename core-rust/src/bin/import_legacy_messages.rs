use libsql::{params, Connection};
use serde_json::{json, Value};
use std::env;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Default)]
struct ImportStats {
    processed: u64,
    imported: u64,
    dropped_unknown_role: u64,
    dropped_non_text: u64,
    dropped_by_substring: u64,
    failed: u64,
}

const EXCLUDE_SUBSTRINGS: &[&str] = &["\"modality\":\"None\"", "Received scheduler notification"];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut source_path = "../core/data/mastra.db".to_string();
    let mut target_path = "./data/core-rust.db".to_string();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" => {
                source_path = args
                    .next()
                    .ok_or("missing value for --source")?
                    .trim()
                    .to_string();
            }
            "--target" => {
                target_path = args
                    .next()
                    .ok_or("missing value for --target")?
                    .trim()
                    .to_string();
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            _ => {
                return Err(format!("unknown arg: {}", arg).into());
            }
        }
    }

    if !Path::new(&source_path).exists() {
        return Err(format!("source db not found: {}", source_path).into());
    }

    if let Some(parent) = Path::new(&target_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let source_db = libsql::Builder::new_local(&source_path).build().await?;
    let target_db = libsql::Builder::new_local(&target_path).build().await?;
    let source = source_db.connect()?;
    let target = target_db.connect()?;

    ensure_target_schema(&target).await?;

    let mut rows = source
        .query(
            "SELECT id, role, createdAt, content FROM mastra_messages ORDER BY createdAt ASC",
            params![],
        )
        .await?;

    target.execute("BEGIN", params![]).await?;

    let mut stats = ImportStats::default();
    while let Some(row) = rows.next().await? {
        stats.processed += 1;

        let role: String = row.get(1)?;
        let created_at: String = row.get(2)?;
        let content: String = row.get(3)?;

        let Some((source_name, role_tag)) = map_role(&role) else {
            stats.dropped_unknown_role += 1;
            continue;
        };

        let Some(text) = extract_event_text(&content) else {
            stats.dropped_non_text += 1;
            continue;
        };

        if let Some(matched) = should_drop_by_substring(&text) {
            eprintln!(
                "IMPORT_DROP_BY_SUBSTRING role={} ts={} matched={}",
                role, created_at, matched
            );
            stats.dropped_by_substring += 1;
            continue;
        }

        let payload_json = serde_json::to_string(&json!({ "text": text }))?;
        let tags_json = serde_json::to_string(&vec!["imported_legacy", role_tag])?;
        let event_id = Uuid::new_v4().to_string();

        if let Err(err) = insert_event(
            &target,
            &event_id,
            &created_at,
            source_name,
            &payload_json,
            &tags_json,
        )
        .await
        {
            eprintln!("IMPORT_ERROR role={} ts={} error={}", role, created_at, err);
            stats.failed += 1;
            continue;
        }

        stats.imported += 1;
    }

    target.execute("COMMIT", params![]).await?;

    println!(
        "IMPORT_RESULT processed={} imported={} dropped_unknown_role={} dropped_non_text={} dropped_by_substring={} failed={}",
        stats.processed,
        stats.imported,
        stats.dropped_unknown_role,
        stats.dropped_non_text,
        stats.dropped_by_substring,
        stats.failed
    );

    Ok(())
}

fn print_help() {
    println!("Import legacy mastra messages into core-rust events");
    println!();
    println!("Usage:");
    println!("  cargo run --bin import_legacy_messages -- [--source <path>] [--target <path>]");
    println!();
    println!("Defaults:");
    println!("  --source ../core/data/mastra.db");
    println!("  --target ./data/core-rust.db");
}

fn map_role(role: &str) -> Option<(&'static str, &'static str)> {
    match role {
        "user" => Some(("user", "user_input")),
        "assistant" => Some(("assistant", "response")),
        "system" => Some(("system", "system_output")),
        _ => None,
    }
}

fn extract_event_text(content: &str) -> Option<String> {
    let value: Value = serde_json::from_str(content).ok()?;
    let parts = value.get("parts").and_then(|v| v.as_array())?;

    let mut chunks: Vec<String> = Vec::new();
    for part in parts {
        if part.get("type").and_then(|v| v.as_str()) != Some("text") {
            continue;
        }
        let Some(raw) = part.get("text").and_then(|v| v.as_str()) else {
            continue;
        };
        let normalized = normalize_text(raw);
        if !normalized.is_empty() {
            chunks.push(normalized);
        }
    }

    if chunks.is_empty() {
        return None;
    }

    Some(chunks.join("\n"))
}

fn normalize_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(content) = value.get("content").and_then(|v| v.as_str()) {
            return content.trim().to_string();
        }
    }

    trimmed.to_string()
}

fn should_drop_by_substring(text: &str) -> Option<&'static str> {
    let lowered = text.to_ascii_lowercase();
    EXCLUDE_SUBSTRINGS.iter().copied().find(|candidate| {
        let candidate_lower = candidate.to_ascii_lowercase();
        lowered.contains(candidate_lower.as_str())
    })
}

async fn ensure_target_schema(
    conn: &Connection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS events (\
        event_id TEXT PRIMARY KEY,\
        ts TEXT NOT NULL,\
        source TEXT NOT NULL,\
        modality TEXT NOT NULL,\
        payload_json TEXT NOT NULL,\
        tags_json TEXT NOT NULL\
      )",
        params![],
    )
    .await?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts)",
        params![],
    )
    .await?;
    Ok(())
}

async fn insert_event(
    conn: &Connection,
    event_id: &str,
    ts: &str,
    source: &str,
    payload_json: &str,
    tags_json: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    conn.execute(
        "INSERT INTO events (event_id, ts, source, modality, payload_json, tags_json) VALUES (?, ?, ?, ?, ?, ?)",
        params![event_id, ts, source, "text", payload_json, tags_json],
    )
    .await?;
    Ok(())
}
