use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};
use time::{format_description, OffsetDateTime};
use tokio::sync::{Mutex, Notify};
use tokio::time::{timeout, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

const DEFAULT_WS_URL: &str = "ws://localhost:2953/";
const DEFAULT_AUTH_TOKEN: &str = "test-token";
const DEFAULT_USER_NAME: &str = "test-user";
const DEFAULT_LOG_DIR: &str = "tests/client/logs";
const DEFAULT_RESPONSE_TIMEOUT_MS: u64 = 60_000;

#[derive(Debug, Deserialize)]
struct Scenario {
    inputs: Vec<InputItem>,
}

#[derive(Debug, Deserialize)]
struct InputItem {
    text: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    timeout_ms: Option<u64>,
}

struct Logger {
    writer: BufWriter<File>,
}

impl Logger {
    fn new(path: &Path) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    fn log(&mut self, entry: Value) -> io::Result<()> {
        let line = serde_json::to_string(&entry)?;
        writeln!(self.writer, "{}", line)?;
        println!("{}", line);
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let scenario_path = parse_args()?;
    let scenario = load_scenario(&scenario_path)?;
    let config = ClientConfig::from_env();
    let log_path = init_log_path(&config.log_dir)?;
    let logger = Arc::new(Mutex::new(Logger::new(&log_path).map_err(|err| err.to_string())?));

    log_event(
        &logger,
        json!({
            "event": "start",
            "time": epoch_ms(),
            "scenario": scenario_path.to_string_lossy(),
        }),
    )
    .await;

    let ws_url = config.ws_url.clone();
    let _ = Url::parse(&ws_url).map_err(|err| format!("invalid WS_URL: {}", err))?;

    log_event(
        &logger,
        json!({
            "event": "connect",
            "time": epoch_ms(),
            "url": ws_url,
        }),
    )
    .await;

    let (ws_stream, _) = connect_async(config.ws_url.clone())
        .await
        .map_err(|err| format!("websocket connect error: {}", err))?;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let auth = format!("{}:{}", config.user_name, config.auth_token);
    ws_sender
        .send(Message::Text(auth.clone().into()))
        .await
        .map_err(|err| format!("auth send failed: {}", err))?;

    log_event(
        &logger,
        json!({
            "event": "auth_sent",
            "time": epoch_ms(),
            "user": config.user_name,
        }),
    )
    .await;

    let reply_count = Arc::new(AtomicUsize::new(0));
    let notify = Arc::new(Notify::new());

    let reader_logger = logger.clone();
    let reader_count = reply_count.clone();
    let reader_notify = notify.clone();

    let reader_task = tokio::spawn(async move {
        while let Some(message) = ws_receiver.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    let raw = text.to_string();
                    let parsed = serde_json::from_str::<Value>(&raw)
                        .unwrap_or_else(|_| Value::String(raw));
                    log_event(
                        &reader_logger,
                        json!({
                            "event": "receive",
                            "time": epoch_ms(),
                            "message": parsed,
                        }),
                    )
                    .await;
                    if is_reply_event(&parsed) {
                        reader_count.fetch_add(1, Ordering::SeqCst);
                        reader_notify.notify_waiters();
                    }
                }
                Ok(Message::Close(frame)) => {
                    log_event(
                        &reader_logger,
                        json!({
                            "event": "close",
                            "time": epoch_ms(),
                            "frame": frame.map(|f| f.reason.to_string()),
                        }),
                    )
                    .await;
                    break;
                }
                Ok(_) => {}
                Err(err) => {
                    log_event(
                        &reader_logger,
                        json!({
                            "event": "error",
                            "time": epoch_ms(),
                            "message": err.to_string(),
                        }),
                    )
                    .await;
                    break;
                }
            }
        }
    });

    for input in scenario.inputs {
        let kind = normalize_kind(input.kind.as_deref())?;
        let payload = json!({
            "type": kind,
            "text": input.text,
        });
        let before = reply_count.load(Ordering::SeqCst);
        ws_sender
            .send(Message::Text(payload.to_string().into()))
            .await
            .map_err(|err| format!("send failed: {}", err))?;

        log_event(
            &logger,
            json!({
                "event": "send",
                "time": epoch_ms(),
                "payload": payload,
            }),
        )
        .await;

        let timeout_ms = input.timeout_ms.unwrap_or(config.response_timeout_ms);
        wait_for_message(&reply_count, &notify, before, timeout_ms).await?;
    }

    let _ = ws_sender.send(Message::Close(None)).await;
    let _ = timeout(Duration::from_secs(2), reader_task).await;

    log_event(
        &logger,
        json!({
            "event": "done",
            "time": epoch_ms(),
        }),
    )
    .await;

    Ok(())
}

struct ClientConfig {
    ws_url: String,
    auth_token: String,
    user_name: String,
    log_dir: PathBuf,
    response_timeout_ms: u64,
}

impl ClientConfig {
    fn from_env() -> Self {
        let ws_url = std::env::var("WS_URL").unwrap_or_else(|_| DEFAULT_WS_URL.to_string());
        let auth_token =
            std::env::var("WEB_AUTH_TOKEN").unwrap_or_else(|_| DEFAULT_AUTH_TOKEN.to_string());
        let user_name = std::env::var("USER_NAME").unwrap_or_else(|_| DEFAULT_USER_NAME.to_string());
        let log_dir = std::env::var("LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_LOG_DIR));
        let response_timeout_ms = std::env::var("RESPONSE_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_RESPONSE_TIMEOUT_MS);

        Self {
            ws_url,
            auth_token,
            user_name,
            log_dir,
            response_timeout_ms,
        }
    }
}

fn parse_args() -> Result<PathBuf, String> {
    let mut args = std::env::args().skip(1);
    let scenario_path = match args.next() {
        Some(value) => value,
        None => return Err(usage()),
    };
    if args.next().is_some() {
        return Err(usage());
    }
    Ok(PathBuf::from(scenario_path))
}

fn usage() -> String {
    "Usage: cargo run --example ws_scenario -- <scenario.yaml>".to_string()
}

fn load_scenario(path: &Path) -> Result<Scenario, String> {
    let raw = fs::read_to_string(path).map_err(|err| format!("failed to read scenario: {}", err))?;
    let scenario: Scenario =
        serde_yaml::from_str(&raw).map_err(|err| format!("failed to parse YAML: {}", err))?;

    if scenario.inputs.is_empty() {
        return Err("scenario inputs must not be empty".to_string());
    }

    for (index, input) in scenario.inputs.iter().enumerate() {
        if input.text.trim().is_empty() {
            return Err(format!("inputs[{}].text must be a non-empty string", index));
        }
        if let Some(kind) = input.kind.as_deref() {
            let normalized = kind.trim();
            if normalized != "message" && normalized != "sensory" {
                return Err(format!(
                    "inputs[{}].type must be 'message' or 'sensory'",
                    index
                ));
            }
        }
        if let Some(timeout_ms) = input.timeout_ms {
            if timeout_ms == 0 {
                return Err(format!(
                    "inputs[{}].timeout_ms must be a positive number",
                    index
                ));
            }
        }
    }

    Ok(scenario)
}

fn normalize_kind(kind: Option<&str>) -> Result<&'static str, String> {
    match kind.map(|value| value.trim()).filter(|value| !value.is_empty()) {
        None => Ok("message"),
        Some("message") => Ok("message"),
        Some("sensory") => Ok("sensory"),
        Some(other) => Err(format!("invalid input type: {}", other)),
    }
}

fn is_reply_event(message: &Value) -> bool {
    let obj = match message.as_object() {
        Some(value) => value,
        None => return false,
    };
    let kind = obj.get("type").and_then(|value| value.as_str());
    if kind != Some("event") {
        return false;
    }
    let event = match obj.get("event").and_then(|value| value.as_object()) {
        Some(value) => value,
        None => return false,
    };
    let tags = match event
        .get("meta")
        .and_then(|value| value.get("tags"))
        .and_then(|value| value.as_array())
    {
        Some(value) => value,
        None => return false,
    };
    let mut has_action = false;
    let mut has_response = false;
    for tag in tags.iter().filter_map(|value| value.as_str()) {
        if tag == "action" {
            has_action = true;
        }
        if tag == "response" {
            has_response = true;
        }
        if has_action && has_response {
            return true;
        }
    }
    false
}

fn init_log_path(log_dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(log_dir).map_err(|err| format!("failed to create log dir: {}", err))?;
    let now = OffsetDateTime::now_utc();
    let format = format_description::parse("[year][month][day]-[hour][minute][second]")
        .map_err(|err| format!("failed to format timestamp: {}", err))?;
    let file_name = format!("{}.jsonl", now.format(&format).unwrap_or_else(|_| "log".to_string()));
    Ok(log_dir.join(file_name))
}

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(StdDuration::from_secs(0))
        .as_millis()
}

async fn log_event(logger: &Arc<Mutex<Logger>>, entry: Value) {
    let mut guard = logger.lock().await;
    if let Err(err) = guard.log(entry) {
        eprintln!("log write failed: {}", err);
    }
}

async fn wait_for_message(
    counter: &AtomicUsize,
    notify: &Notify,
    before: usize,
    timeout_ms: u64,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let current = counter.load(Ordering::SeqCst);
        if current > before {
            return Ok(());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for server response after {}ms",
                timeout_ms
            ));
        }
        let remaining = deadline - now;
        if timeout(remaining, notify.notified()).await.is_err() {
            return Err(format!(
                "timed out waiting for server response after {}ms",
                timeout_ms
            ));
        }
    }
}
