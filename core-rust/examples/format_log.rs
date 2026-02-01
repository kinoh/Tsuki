use serde_json::Value;
use std::fs;

fn main() {
    if let Err(err) = run() {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or_else(usage)?;
    let content = fs::read_to_string(&path).map_err(|err| format!("failed to read log: {}", err))?;

    let mut last_send_time: Option<i64> = None;

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let parsed: Value =
            serde_json::from_str(line).map_err(|err| format!("invalid jsonl: {}", err))?;
        let event = parsed.get("event").and_then(|value| value.as_str());

        if event == Some("send") {
            let payload = parsed.get("payload");
            let kind = payload
                .and_then(|value| value.get("type"))
                .and_then(|value| value.as_str())
                .unwrap_or("message");
            let label = if kind == "message" { "user" } else { kind };
            let text = payload
                .and_then(|value| value.get("text"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            println!("{}: {}", label, text);
            last_send_time = parsed.get("time").and_then(|value| value.as_i64());
            continue;
        }

        if event == Some("receive") {
            let message = parsed.get("message");
            if let Some((label, text)) = extract_event_text(message) {
                if let (Some(send_time), Some(receive_time)) = (
                    last_send_time,
                    parsed.get("time").and_then(|value| value.as_i64()),
                ) {
                    println!("{}: {} ({}ms)", label, text, receive_time - send_time);
                } else {
                    println!("{}: {}", label, text);
                }
            } else if let Some(message) = message {
                println!("receive: {}", message);
            }
        }
    }

    Ok(())
}

fn extract_event_text(message: Option<&Value>) -> Option<(String, String)> {
    let message = message?;
    let event = message.get("event")?;
    let source = event
        .get("source")
        .and_then(|value| value.as_str())
        .unwrap_or("event");
    let text = event
        .get("payload")
        .and_then(|value| value.get("text"))
        .and_then(|value| value.as_str())?;
    Some((source.to_string(), text.to_string()))
}

fn usage() -> String {
    "Usage: cargo run --example format_log -- <log.jsonl>".to_string()
}
