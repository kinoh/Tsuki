use async_openai::{
    config::OpenAIConfig,
    types::responses::{CreateResponseArgs, InputParam},
    Client,
};
use futures::{SinkExt, StreamExt};
use libsql::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use time::{format_description, OffsetDateTime};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const DEFAULT_SCENARIO_PATH: &str = "tests/integration/scenarios/example.yaml";
const DEFAULT_RUNNER_CONFIG_PATH: &str = "tests/integration/config/runner.toml";
const DEFAULT_AUTH_TOKEN: &str = "test-token";
const DEFAULT_USER_NAME: &str = "integration-tester";
const READY_TIMEOUT_MS: u64 = 90_000;
const READY_INTERVAL_MS: u64 = 500;
const REQUIRED_BASELINE_METRICS: [&str; 2] = ["scenario_requirement_fit", "dialog_naturalness"];

#[derive(Debug, Clone)]
struct Args {
    scenario_path: PathBuf,
    runner_config_path: PathBuf,
    run_count_override: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunnerConfig {
    tester: RoleConfig,
    judge: RoleConfig,
    execution: ExecutionConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct RoleConfig {
    model: String,
    prompt_file: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ExecutionConfig {
    run_count: usize,
    max_turns: usize,
    turn_timeout_ms: u64,
    scenario_timeout_ms: u64,
    transient_retry: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct Scenario {
    name: String,
    #[serde(default)]
    include_debug_events: bool,
    tester_instructions: String,
    metrics_definition: HashMap<String, MetricDefinition>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MetricDefinition {
    description: String,
}

#[derive(Debug, Clone, Serialize)]
struct TranscriptTurn {
    user: String,
    assistant: String,
}

#[derive(Debug, Clone, Serialize)]
struct RunResult {
    run_index: usize,
    pass: bool,
    failure_code: Option<String>,
    failure_detail: Option<String>,
    metrics: HashMap<String, f64>,
    judge_summary: Option<String>,
    turn_count: usize,
    event_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct AggregateMetric {
    mean: f64,
    min: f64,
}

#[derive(Debug, Clone, Serialize)]
struct IntegrationResult {
    scenario_name: String,
    scenario_path: String,
    runner_config_path: String,
    run_count: usize,
    overall_pass: bool,
    gates: HashMap<String, AggregateMetric>,
    runs: Vec<RunResult>,
    generated_at: String,
    db_path: String,
    ws_url: String,
}

#[derive(Debug, Clone)]
struct RuntimeContext {
    temp_dir: PathBuf,
    db_path: PathBuf,
    ws_url: String,
    auth_token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct JudgeOutput {
    metrics: HashMap<String, f64>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    pass: Option<bool>,
}

#[derive(Debug, Clone)]
struct RoleRuntime {
    model: String,
    prompt: String,
}

#[derive(Debug, Clone)]
struct TestAssets {
    scenario_path: PathBuf,
    runner_config_path: PathBuf,
    scenario: Scenario,
    tester: RoleRuntime,
    judge: RoleRuntime,
    execution: ExecutionConfig,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let Some(args) = parse_args()? else {
        println!("{}", usage());
        return Ok(());
    };
    let assets = load_assets(&args)?;
    ensure_openai_key()?;

    let runtime = prepare_runtime()?;
    let mut core = start_core(&runtime).await?;
    wait_for_ws(&runtime.ws_url, &mut core).await?;

    let result = execute_runs(&assets, &runtime).await;
    let _ = core.kill().await;

    let result = result?;
    let output_path = write_result_artifact(&result)?;

    println!("integration result: {}", output_path.display());
    println!(
        "overall_pass={} run_count={}",
        result.overall_pass, result.run_count
    );

    Ok(())
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut scenario_path = PathBuf::from(DEFAULT_SCENARIO_PATH);
    let mut runner_config_path = PathBuf::from(DEFAULT_RUNNER_CONFIG_PATH);
    let mut run_count_override: Option<usize> = None;

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--scenario" => {
                let value = it.next().ok_or("--scenario requires a path")?;
                scenario_path = PathBuf::from(value);
            }
            "--config" => {
                let value = it.next().ok_or("--config requires a path")?;
                runner_config_path = PathBuf::from(value);
            }
            "--run-count" => {
                let value = it.next().ok_or("--run-count requires a number")?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --run-count: {}", value))?;
                if parsed == 0 {
                    return Err("--run-count must be >= 1".to_string());
                }
                run_count_override = Some(parsed);
            }
            "-h" | "--help" => {
                return Ok(None);
            }
            _ => {
                return Err(format!("unknown argument: {}\n{}", arg, usage()));
            }
        }
    }

    Ok(Some(Args {
        scenario_path,
        runner_config_path,
        run_count_override,
    }))
}

fn usage() -> String {
    "Usage: cargo run --example integration_harness -- [--scenario <path>] [--config <path>] [--run-count <n>]".to_string()
}

fn load_assets(args: &Args) -> Result<TestAssets, String> {
    let runner_raw = fs::read_to_string(&args.runner_config_path)
        .map_err(|err| format!("failed to read runner config: {}", err))?;
    let mut runner: RunnerConfig = toml::from_str(&runner_raw)
        .map_err(|err| format!("failed to parse runner config: {}", err))?;

    if let Some(value) = args.run_count_override {
        runner.execution.run_count = value;
    }

    let scenario_raw = fs::read_to_string(&args.scenario_path)
        .map_err(|err| format!("failed to read scenario: {}", err))?;
    let scenario: Scenario = serde_yaml::from_str(&scenario_raw)
        .map_err(|err| format!("failed to parse scenario: {}", err))?;

    for key in REQUIRED_BASELINE_METRICS {
        if !scenario.metrics_definition.contains_key(key) {
            return Err(format!(
                "scenario missing required common metric '{}': {}",
                key,
                args.scenario_path.display()
            ));
        }
    }

    let tester_prompt = fs::read_to_string(&runner.tester.prompt_file).map_err(|err| {
        format!(
            "failed to read tester prompt '{}': {}",
            runner.tester.prompt_file, err
        )
    })?;
    let judge_prompt = fs::read_to_string(&runner.judge.prompt_file).map_err(|err| {
        format!(
            "failed to read judge prompt '{}': {}",
            runner.judge.prompt_file, err
        )
    })?;

    Ok(TestAssets {
        scenario_path: args.scenario_path.clone(),
        runner_config_path: args.runner_config_path.clone(),
        scenario,
        tester: RoleRuntime {
            model: runner.tester.model,
            prompt: tester_prompt,
        },
        judge: RoleRuntime {
            model: runner.judge.model,
            prompt: judge_prompt,
        },
        execution: runner.execution,
    })
}

fn ensure_openai_key() -> Result<(), String> {
    match std::env::var("OPENAI_API_KEY") {
        Ok(value) if !value.trim().is_empty() => Ok(()),
        _ => Err("OPENAI_API_KEY is required".to_string()),
    }
}

fn prepare_runtime() -> Result<RuntimeContext, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base_config_path = manifest_dir.join("config.toml");
    let base_config_raw = fs::read_to_string(&base_config_path)
        .map_err(|err| format!("failed to read base config: {}", err))?;
    let mut root: toml::Value = toml::from_str(&base_config_raw)
        .map_err(|err| format!("failed to parse base config: {}", err))?;

    let now = OffsetDateTime::now_utc();
    let temp_dir = std::env::temp_dir().join(format!(
        "tsuki-core-rust-integration-{}-{}",
        now.unix_timestamp(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&temp_dir).map_err(|err| format!("failed to create temp dir: {}", err))?;

    let db_path = temp_dir.join("integration.db");
    let port = pick_free_port()?;

    let db_table = root
        .get_mut("db")
        .and_then(toml::Value::as_table_mut)
        .ok_or("config.toml missing [db]")?;
    db_table.insert(
        "path".to_string(),
        toml::Value::String(db_path.to_string_lossy().to_string()),
    );

    let server_table = root
        .get_mut("server")
        .and_then(toml::Value::as_table_mut)
        .ok_or("config.toml missing [server]")?;
    server_table.insert("port".to_string(), toml::Value::Integer(port as i64));

    let patched =
        toml::to_string(&root).map_err(|err| format!("failed to render config: {}", err))?;
    let config_path = temp_dir.join("config.toml");
    fs::write(&config_path, patched)
        .map_err(|err| format!("failed to write temp config: {}", err))?;

    let auth_token =
        std::env::var("WEB_AUTH_TOKEN").unwrap_or_else(|_| DEFAULT_AUTH_TOKEN.to_string());
    let ws_url = format!("ws://127.0.0.1:{}/", port);

    Ok(RuntimeContext {
        temp_dir,
        db_path,
        ws_url,
        auth_token,
    })
}

fn pick_free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|err| format!("failed to bind random port: {}", err))?
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|err| format!("failed to inspect local addr: {}", err))
}

async fn start_core(runtime: &RuntimeContext) -> Result<Child, String> {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut command = Command::new("cargo");
    sanitize_cargo_env(&mut command);
    command
        .args([
            "run",
            "--manifest-path",
            manifest_path.to_string_lossy().as_ref(),
            "--bin",
            "tsuki-core-rust",
        ])
        .current_dir(&runtime.temp_dir)
        .env("WEB_AUTH_TOKEN", runtime.auth_token.as_str())
        .env("MEMGRAPH_URI", "bolt://localhost:7697")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| format!("failed to spawn core-rust: {}", err))
}

fn sanitize_cargo_env(command: &mut Command) {
    let mut keys = Vec::new();
    for (key, _) in std::env::vars() {
        if key == "CARGO_MANIFEST_DIR"
            || key == "CARGO_BIN_NAME"
            || key == "CARGO_CRATE_NAME"
            || key.starts_with("CARGO_PKG_")
        {
            keys.push(key);
        }
    }
    for key in keys {
        command.env_remove(key);
    }
}

async fn wait_for_ws(ws_url: &str, core_proc: &mut Child) -> Result<(), String> {
    let url = url::Url::parse(ws_url).map_err(|err| format!("invalid ws url: {}", err))?;
    let host = url.host_str().ok_or("ws url host missing")?.to_string();
    let port = match (url.port(), url.scheme()) {
        (Some(value), _) => value,
        (None, "wss") => 443,
        (None, _) => 80,
    };

    let start = Instant::now();
    while start.elapsed().as_millis() < READY_TIMEOUT_MS as u128 {
        if let Ok(Some(status)) = core_proc.try_wait() {
            return Err(format!("core-rust exited early: {}", status));
        }
        if try_connect(&host, port).await {
            return Ok(());
        }
        sleep(Duration::from_millis(READY_INTERVAL_MS)).await;
    }

    Err(format!("timed out waiting for ws readiness: {}", ws_url))
}

async fn try_connect(host: &str, port: u16) -> bool {
    let addr = format!("{}:{}", host, port);
    matches!(
        timeout(Duration::from_secs(1), TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

async fn execute_runs(
    assets: &TestAssets,
    runtime: &RuntimeContext,
) -> Result<IntegrationResult, String> {
    let run_count = assets.execution.run_count;
    let mut runs = Vec::with_capacity(run_count);

    for run_index in 1..=run_count {
        let before_count = read_event_count(&runtime.db_path).await?;
        let mut transcript: Vec<TranscriptTurn> = Vec::new();
        let mut convo_error: Option<(String, String)> = None;
        let max_attempts = assets.execution.transient_retry.saturating_add(1);

        for attempt in 1..=max_attempts {
            let convo = timeout(
                Duration::from_millis(assets.execution.scenario_timeout_ms),
                run_tester_dialogue(assets, runtime),
            )
            .await;

            match convo {
                Ok(Ok(value)) => {
                    transcript = value;
                    convo_error = None;
                    break;
                }
                Ok(Err(err)) => {
                    convo_error = Some(("EXEC_WS_ERROR".to_string(), err));
                }
                Err(_) => {
                    convo_error = Some((
                        "EXEC_TIMEOUT".to_string(),
                        "scenario execution timed out".to_string(),
                    ));
                }
            }

            if attempt < max_attempts {
                sleep(Duration::from_millis(300)).await;
            }
        }

        let raw_events = read_events_since(&runtime.db_path, before_count).await?;
        let filtered_events = filter_events(raw_events, assets.scenario.include_debug_events);

        if let Some((code, detail)) = convo_error {
            runs.push(RunResult {
                run_index,
                pass: false,
                failure_code: Some(code),
                failure_detail: Some(detail),
                metrics: HashMap::new(),
                judge_summary: None,
                turn_count: transcript.len(),
                event_count: filtered_events.len(),
            });
            continue;
        }

        let judge_out = judge_run(assets, &transcript, &filtered_events).await;
        match judge_out {
            Ok(output) => {
                let validation = validate_metrics(&assets.scenario, &output.metrics);
                if let Err(err) = validation {
                    runs.push(RunResult {
                        run_index,
                        pass: false,
                        failure_code: Some("INVALID_OUTPUT".to_string()),
                        failure_detail: Some(err),
                        metrics: output.metrics,
                        judge_summary: output.summary,
                        turn_count: transcript.len(),
                        event_count: filtered_events.len(),
                    });
                    continue;
                }

                runs.push(RunResult {
                    run_index,
                    pass: output.pass.unwrap_or(false),
                    failure_code: None,
                    failure_detail: None,
                    metrics: output.metrics,
                    judge_summary: output.summary,
                    turn_count: transcript.len(),
                    event_count: filtered_events.len(),
                });
            }
            Err(err) => {
                runs.push(RunResult {
                    run_index,
                    pass: false,
                    failure_code: Some("JUDGE_ERROR".to_string()),
                    failure_detail: Some(err),
                    metrics: HashMap::new(),
                    judge_summary: None,
                    turn_count: transcript.len(),
                    event_count: filtered_events.len(),
                });
            }
        }
    }

    let metric_keys = collect_metric_keys(&assets.scenario);
    let gates = compute_gates(&runs, &metric_keys)?;
    let overall_pass = evaluate_overall_pass(&runs, &gates, &metric_keys);

    Ok(IntegrationResult {
        scenario_name: assets.scenario.name.clone(),
        scenario_path: assets.scenario_path.to_string_lossy().to_string(),
        runner_config_path: assets.runner_config_path.to_string_lossy().to_string(),
        run_count,
        overall_pass,
        gates,
        runs,
        generated_at: OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "".to_string()),
        db_path: runtime.db_path.to_string_lossy().to_string(),
        ws_url: runtime.ws_url.clone(),
    })
}

fn evaluate_overall_pass(
    runs: &[RunResult],
    gates: &HashMap<String, AggregateMetric>,
    metric_keys: &[String],
) -> bool {
    if runs.iter().any(|run| run.failure_code.is_some()) {
        return false;
    }

    for key in metric_keys {
        let Some(metric) = gates.get(key.as_str()) else {
            return false;
        };
        if metric.mean <= 0.7 || metric.min <= 0.5 {
            return false;
        }
    }
    true
}

fn compute_gates(
    runs: &[RunResult],
    metric_keys: &[String],
) -> Result<HashMap<String, AggregateMetric>, String> {
    let mut gates = HashMap::new();

    for key in metric_keys {
        let mut values = Vec::new();
        for run in runs {
            if let Some(value) = run.metrics.get(key.as_str()) {
                values.push(*value);
            }
        }
        if values.len() != runs.len() {
            return Err(format!("missing metric '{}' in one or more runs", key));
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let min = values.iter().fold(
            f64::INFINITY,
            |acc, value| if *value < acc { *value } else { acc },
        );
        gates.insert(
            key.to_string(),
            AggregateMetric {
                mean: round3(mean),
                min: round3(min),
            },
        );
    }

    Ok(gates)
}

fn collect_metric_keys(scenario: &Scenario) -> Vec<String> {
    let mut keys = scenario
        .metrics_definition
        .keys()
        .cloned()
        .collect::<Vec<String>>();
    keys.sort();
    keys
}

fn validate_metrics(scenario: &Scenario, metrics: &HashMap<String, f64>) -> Result<(), String> {
    for key in scenario.metrics_definition.keys() {
        let Some(value) = metrics.get(key) else {
            return Err(format!("judge output missing metric: {}", key));
        };
        if *value < 0.0 || *value > 1.0 || !value.is_finite() {
            return Err(format!("metric '{}' must be finite and in [0,1]", key));
        }
    }
    Ok(())
}

async fn run_tester_dialogue(
    assets: &TestAssets,
    runtime: &RuntimeContext,
) -> Result<Vec<TranscriptTurn>, String> {
    let (mut ws_stream, _) = connect_async(runtime.ws_url.as_str())
        .await
        .map_err(|err| format!("websocket connect failed: {}", err))?;

    let auth = format!("{}:{}", DEFAULT_USER_NAME, runtime.auth_token);
    ws_stream
        .send(Message::Text(auth.into()))
        .await
        .map_err(|err| format!("auth send failed: {}", err))?;

    let mut transcript: Vec<TranscriptTurn> = Vec::new();

    for _ in 0..assets.execution.max_turns {
        let utterance = generate_tester_utterance(assets, &transcript).await?;
        if utterance == "__TEST_DONE__" {
            break;
        }
        if utterance.trim().is_empty() {
            return Err("tester returned empty utterance".to_string());
        }

        let payload = json!({
            "type": "message",
            "text": utterance,
        });
        ws_stream
            .send(Message::Text(payload.to_string().into()))
            .await
            .map_err(|err| format!("message send failed: {}", err))?;

        let assistant =
            wait_for_reply_text(&mut ws_stream, assets.execution.turn_timeout_ms).await?;
        transcript.push(TranscriptTurn {
            user: payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            assistant,
        });
    }

    let _ = ws_stream.send(Message::Close(None)).await;

    if transcript.is_empty() {
        return Err("tester produced no conversation turns".to_string());
    }

    Ok(transcript)
}

async fn generate_tester_utterance(
    assets: &TestAssets,
    transcript: &[TranscriptTurn],
) -> Result<String, String> {
    let transcript_json = serde_json::to_string_pretty(transcript)
        .map_err(|err| format!("failed to serialize transcript: {}", err))?;
    let input = format!(
        "Scenario instructions:\n{}\n\nConversation transcript:\n{}\n\nIf scenario goals are already sufficiently exercised, output exactly __TEST_DONE__. Otherwise output exactly one next user utterance.",
        assets.scenario.tester_instructions, transcript_json
    );

    let response = call_llm(
        assets.tester.model.as_str(),
        assets.tester.prompt.as_str(),
        input.as_str(),
    )
    .await?;

    let line = response
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    if line.is_empty() {
        return Err("tester response is empty".to_string());
    }
    Ok(line)
}

async fn wait_for_reply_text(
    ws_stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    timeout_ms: u64,
) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for reply after {}ms",
                timeout_ms
            ));
        }

        let remaining = deadline - now;
        let message = timeout(remaining, ws_stream.next())
            .await
            .map_err(|_| format!("timed out waiting for reply after {}ms", timeout_ms))?;

        match message {
            Some(Ok(Message::Text(text))) => {
                let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| Value::Null);
                if is_reply_event(&parsed) {
                    return Ok(extract_reply_text(&parsed));
                }
            }
            Some(Ok(Message::Close(_))) => return Err("websocket closed before reply".to_string()),
            Some(Ok(_)) => {}
            Some(Err(err)) => return Err(format!("websocket receive failed: {}", err)),
            None => return Err("websocket stream ended before reply".to_string()),
        }
    }
}

fn is_reply_event(message: &Value) -> bool {
    let obj = match message.as_object() {
        Some(value) => value,
        None => return false,
    };
    if obj.get("type").and_then(Value::as_str) != Some("event") {
        return false;
    }
    let event = match obj.get("event").and_then(Value::as_object) {
        Some(value) => value,
        None => return false,
    };
    let tags = match event
        .get("meta")
        .and_then(|value| value.get("tags"))
        .and_then(Value::as_array)
    {
        Some(value) => value,
        None => return false,
    };

    let mut has_action = false;
    let mut has_response = false;
    for tag in tags.iter().filter_map(Value::as_str) {
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

fn extract_reply_text(message: &Value) -> String {
    message
        .get("event")
        .and_then(|value| value.get("payload"))
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "(missing text payload)".to_string())
}

async fn read_event_count(db_path: &Path) -> Result<usize, String> {
    let db = libsql::Builder::new_local(db_path.to_string_lossy().as_ref())
        .build()
        .await
        .map_err(|err| format!("failed to open db: {}", err))?;
    let conn = db
        .connect()
        .map_err(|err| format!("failed to connect db: {}", err))?;
    let mut rows = conn
        .query("SELECT COUNT(*) FROM events", params![])
        .await
        .map_err(|err| format!("failed to query event count: {}", err))?;
    if let Some(row) = rows.next().await.map_err(|err| err.to_string())? {
        let count: i64 = row.get(0).map_err(|err| err.to_string())?;
        return Ok(count.max(0) as usize);
    }
    Ok(0)
}

async fn read_events_since(db_path: &Path, offset: usize) -> Result<Vec<Value>, String> {
    let db = libsql::Builder::new_local(db_path.to_string_lossy().as_ref())
        .build()
        .await
        .map_err(|err| format!("failed to open db: {}", err))?;
    let conn = db
        .connect()
        .map_err(|err| format!("failed to connect db: {}", err))?;
    let mut rows = conn
        .query(
            "SELECT ts, source, modality, payload_json, tags_json FROM events ORDER BY ts ASC LIMIT -1 OFFSET ?",
            params![offset as i64],
        )
        .await
        .map_err(|err| format!("failed to query events: {}", err))?;

    let mut out = Vec::new();
    while let Some(row) = rows.next().await.map_err(|err| err.to_string())? {
        let ts: String = row.get(0).map_err(|err| err.to_string())?;
        let source: String = row.get(1).map_err(|err| err.to_string())?;
        let modality: String = row.get(2).map_err(|err| err.to_string())?;
        let payload_json: String = row.get(3).map_err(|err| err.to_string())?;
        let tags_json: String = row.get(4).map_err(|err| err.to_string())?;

        let payload = serde_json::from_str::<Value>(&payload_json).unwrap_or(Value::Null);
        let tags = serde_json::from_str::<Vec<String>>(&tags_json).unwrap_or_default();
        let compact_payload = compact_payload(&payload);

        out.push(json!({
            "ts": ts,
            "source": source,
            "modality": modality,
            "tags": tags,
            "payload": compact_payload,
        }));
    }

    Ok(out)
}

fn compact_payload(payload: &Value) -> Value {
    let mut out = Map::new();

    if let Some(text) = payload.get("text").and_then(Value::as_str) {
        out.insert("text".to_string(), Value::String(text.to_string()));
    }
    if let Some(target) = payload.get("target").and_then(Value::as_str) {
        out.insert("target".to_string(), Value::String(target.to_string()));
    }

    if out.is_empty() {
        if let Some(obj) = payload.as_object() {
            for (key, value) in obj.iter() {
                if key == "raw" {
                    continue;
                }
                out.insert(key.clone(), value.clone());
            }
        }
    }

    Value::Object(out)
}

fn filter_events(events: Vec<Value>, include_debug_events: bool) -> Vec<Value> {
    if include_debug_events {
        return events;
    }

    events
        .into_iter()
        .filter(|event| {
            let tags = event
                .get("tags")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                .unwrap_or_default();
            !tags
                .iter()
                .any(|tag| *tag == "debug" || tag.starts_with("llm."))
        })
        .collect()
}

async fn judge_run(
    assets: &TestAssets,
    transcript: &[TranscriptTurn],
    events: &[Value],
) -> Result<JudgeOutput, String> {
    let metrics_json = serde_json::to_string_pretty(&assets.scenario.metrics_definition)
        .map_err(|err| format!("failed to serialize metrics_definition: {}", err))?;
    let transcript_json = serde_json::to_string_pretty(transcript)
        .map_err(|err| format!("failed to serialize transcript: {}", err))?;
    let events_json = serde_json::to_string_pretty(events)
        .map_err(|err| format!("failed to serialize events: {}", err))?;

    let input = format!(
        "Scenario tester_instructions:\n{}\n\nMetrics definition:\n{}\n\nTranscript:\n{}\n\nEvents:\n{}\n\nReturn JSON only.",
        assets.scenario.tester_instructions, metrics_json, transcript_json, events_json
    );

    let raw = call_llm(
        assets.judge.model.as_str(),
        assets.judge.prompt.as_str(),
        input.as_str(),
    )
    .await?;

    let json_text = extract_json_object(&raw)
        .ok_or_else(|| format!("judge output is not valid JSON object: {}", raw))?;
    serde_json::from_str::<JudgeOutput>(json_text.as_str())
        .map_err(|err| format!("failed to parse judge JSON: {}", err))
}

async fn call_llm(model: &str, instructions: &str, input: &str) -> Result<String, String> {
    let client = Client::<OpenAIConfig>::new();
    let req = CreateResponseArgs::default()
        .model(model)
        .instructions(instructions)
        .input(InputParam::Text(input.to_string()))
        .build()
        .map_err(|err| format!("failed to build llm request: {}", err))?;

    let resp = client
        .responses()
        .create(req)
        .await
        .map_err(|err| format!("llm request failed: {}", err))?;

    Ok(resp
        .output_text()
        .unwrap_or_else(|| "".to_string())
        .trim()
        .to_string())
}

fn extract_json_object(text: &str) -> Option<String> {
    let mut start = None;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            continue;
        }

        if ch == '{' {
            if start.is_none() {
                start = Some(idx);
            }
            depth += 1;
            continue;
        }

        if ch == '}' {
            depth -= 1;
            if depth == 0 {
                if let Some(begin) = start {
                    return Some(text[begin..=idx].to_string());
                }
            }
        }
    }
    None
}

fn round3(value: f64) -> f64 {
    let factor = 1000.0;
    (value * factor).round() / factor
}

fn write_result_artifact(result: &IntegrationResult) -> Result<PathBuf, String> {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/integration/results");
    fs::create_dir_all(&base_dir)
        .map_err(|err| format!("failed to create results dir: {}", err))?;

    let ts_format = format_description::parse("[year][month][day]-[hour][minute][second]")
        .map_err(|err| format!("failed to parse time format: {}", err))?;
    let stamp = OffsetDateTime::now_utc()
        .format(&ts_format)
        .unwrap_or_else(|_| "result".to_string());
    let safe_name = result
        .scenario_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    let path = base_dir.join(format!("{}__{}.json", stamp, safe_name));

    let body = serde_json::to_string_pretty(result)
        .map_err(|err| format!("failed to serialize result: {}", err))?;
    fs::write(&path, body).map_err(|err| format!("failed to write result artifact: {}", err))?;
    Ok(path)
}
