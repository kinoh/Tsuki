use async_openai::{
    config::OpenAIConfig,
    types::responses::{CreateResponseArgs, InputParam},
    Client,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use bech32::{ToBase32, Variant};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use time::{format_description, OffsetDateTime};
use tokio::io::AsyncWriteExt;
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
const SECRET_DIR_PATH: &str = "tests/integration/secrets";
const SECRET_KEY_ENV_VAR: &str = "PROMPT_PRIVATE_KEY";
const INCOMPLETE_SCENARIO_REQUIREMENT_CAP: f64 = 0.5;
const DEFAULT_EMIT_WAIT_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_TRIGGER_WAIT_TAGS: [&str; 2] = [
    "self_improvement.module_processed",
    "self_improvement.trigger_processed",
];

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
    core: CoreConfig,
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
struct CoreConfig {
    #[serde(default)]
    prompts_file: Option<String>,
    memgraph_uri: String,
    memgraph_backup_path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Scenario {
    name: String,
    #[serde(default)]
    include_debug_events: bool,
    #[serde(default)]
    steps: Vec<ScenarioStep>,
    metrics_definition: HashMap<String, MetricDefinition>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MetricDefinition {
    description: String,
    #[serde(default)]
    exclude_from_pass: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ScenarioStep {
    Conversation {
        tester_instructions: String,
        #[serde(default)]
        max_turns: Option<usize>,
    },
    EmitEvent {
        event: EmitEventPayload,
        #[serde(default)]
        wait_for: Option<WaitForSpec>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct EmitEventPayload {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WaitForSpec {
    #[serde(default)]
    tags_any: Vec<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
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
    response_time_ms_mean: Option<f64>,
    response_time_ms_min: Option<u64>,
    response_time_ms_max: Option<u64>,
    response_time_ms_by_turn: Vec<u64>,
    message_log: Vec<TranscriptTurn>,
    log_file: Option<String>,
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
    memgraph_uri: String,
    memgraph_backup_path: String,
}

#[derive(Debug, Clone)]
struct RuntimeContext {
    temp_dir: PathBuf,
    db_path: PathBuf,
    ws_url: String,
    auth_token: String,
    memgraph_uri: String,
    memgraph_backup_path: PathBuf,
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
struct TurnReply {
    assistant: String,
    saw_decision: bool,
}

#[derive(Debug, Clone)]
struct DialogueRun {
    transcript: Vec<TranscriptTurn>,
    response_times_ms: Vec<u64>,
    events: Vec<Value>,
}

#[derive(Debug, Clone)]
struct ConversationRuntimeStep {
    tester_instructions: String,
    max_turns: usize,
}

#[derive(Debug, Clone)]
struct EmitEventRuntimeStep {
    event: EmitEventPayload,
    wait_for_tags_any: Vec<String>,
    wait_for_timeout_ms: u64,
}

#[derive(Debug, Clone)]
enum RuntimeScenarioStep {
    Conversation(ConversationRuntimeStep),
    EmitEvent(EmitEventRuntimeStep),
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
    core: CoreConfig,
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
    ensure_openai_key()?;
    let secret_identity = load_secret_identity_from_env()?;
    let assets = load_assets(&args, &secret_identity)?;

    let runtime = prepare_runtime(&assets.core)?;
    restore_memgraph_snapshot(&runtime).await?;
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

fn resolve_to_manifest_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

#[derive(Debug, Deserialize)]
struct PromptPrivateKeyJwk {
    kty: String,
    crv: String,
    d: String,
}

fn load_secret_identity_from_env() -> Result<age::x25519::Identity, String> {
    let key_raw = std::env::var(SECRET_KEY_ENV_VAR).map_err(|_| {
        format!(
            "{} is required for decrypting scenario secret placeholders",
            SECRET_KEY_ENV_VAR
        )
    })?;
    if key_raw.trim().is_empty() {
        return Err(format!("{} must not be empty", SECRET_KEY_ENV_VAR));
    }

    let jwk: PromptPrivateKeyJwk = serde_json::from_str(&key_raw)
        .map_err(|err| format!("{} must be valid JWK JSON: {}", SECRET_KEY_ENV_VAR, err))?;
    if jwk.kty != "OKP" || jwk.crv != "X25519" {
        return Err(format!(
            "{} must be an X25519 JWK (kty=OKP, crv=X25519)",
            SECRET_KEY_ENV_VAR
        ));
    }

    let key_bytes = URL_SAFE_NO_PAD
        .decode(jwk.d.as_bytes())
        .map_err(|err| format!("failed to decode JWK 'd' as base64url: {}", err))?;
    let key_array: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "JWK 'd' must decode to 32 bytes".to_string())?;

    let encoded = bech32::encode("age-secret-key-", key_array.to_base32(), Variant::Bech32)
        .map_err(|err| format!("failed to encode age secret key: {}", err))?;
    age::x25519::Identity::from_str(&encoded)
        .map_err(|err| format!("failed to construct age identity from JWK: {}", err))
}

fn load_scenario_with_secrets(
    path: &Path,
    identity: &age::x25519::Identity,
) -> Result<Scenario, String> {
    let scenario_raw =
        fs::read_to_string(path).map_err(|err| format!("failed to read scenario: {}", err))?;
    let mut scenario_yaml: serde_yaml::Value = serde_yaml::from_str(&scenario_raw)
        .map_err(|err| format!("failed to parse scenario yaml: {}", err))?;
    let mut cache = HashMap::<String, String>::new();
    apply_secret_templates(&mut scenario_yaml, identity, &mut cache)?;
    serde_yaml::from_value::<Scenario>(scenario_yaml)
        .map_err(|err| format!("failed to parse scenario after template expansion: {}", err))
}

fn apply_secret_templates(
    value: &mut serde_yaml::Value,
    identity: &age::x25519::Identity,
    cache: &mut HashMap<String, String>,
) -> Result<(), String> {
    match value {
        serde_yaml::Value::String(text) => {
            *text = resolve_secret_placeholders(text, identity, cache)?;
            Ok(())
        }
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                apply_secret_templates(item, identity, cache)?;
            }
            Ok(())
        }
        serde_yaml::Value::Mapping(map) => {
            for (_, entry) in map.iter_mut() {
                apply_secret_templates(entry, identity, cache)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn resolve_secret_placeholders(
    text: &str,
    identity: &age::x25519::Identity,
    cache: &mut HashMap<String, String>,
) -> Result<String, String> {
    let mut out = String::new();
    let mut index = 0usize;

    while let Some(start_rel) = text[index..].find("{{") {
        let start = index + start_rel;
        out.push_str(&text[index..start]);
        let after = start + 2;
        let Some(end_rel) = text[after..].find("}}") else {
            return Err(format!("unclosed secret placeholder in text: {}", text));
        };
        let end = after + end_rel;
        let name = text[after..end].trim();
        let secret = decrypt_secret_placeholder(name, identity, cache)?;
        out.push_str(secret.as_str());
        index = end + 2;
    }

    out.push_str(&text[index..]);
    Ok(out)
}

fn decrypt_secret_placeholder(
    name: &str,
    identity: &age::x25519::Identity,
    cache: &mut HashMap<String, String>,
) -> Result<String, String> {
    if let Some(value) = cache.get(name) {
        return Ok(value.clone());
    }
    if name.is_empty() {
        return Err("empty secret placeholder is not allowed".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    {
        return Err(format!("invalid secret placeholder name: {}", name));
    }

    let secret_path =
        resolve_to_manifest_path(PathBuf::from(SECRET_DIR_PATH)).join(format!("{}.age", name));
    if !secret_path.exists() {
        return Err(format!(
            "secret file not found for placeholder '{}': {}",
            name,
            secret_path.display()
        ));
    }

    let ciphertext = fs::read(&secret_path)
        .map_err(|err| format!("failed to read secret file '{}': {}", name, err))?;
    let decryptor = age::Decryptor::new(&ciphertext[..])
        .map_err(|err| format!("failed to parse age ciphertext '{}': {}", name, err))?;
    let mut reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|err| format!("failed to decrypt secret '{}': {}", name, err))?;
    let mut plaintext = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut plaintext)
        .map_err(|err| format!("failed to read decrypted secret '{}': {}", name, err))?;
    let decrypted = String::from_utf8(plaintext)
        .map_err(|err| format!("secret '{}' is not valid utf-8: {}", name, err))?;
    let decrypted = decrypted.trim_end_matches('\n').to_string();
    cache.insert(name.to_string(), decrypted.clone());
    Ok(decrypted)
}

fn load_assets(args: &Args, identity: &age::x25519::Identity) -> Result<TestAssets, String> {
    let runner_raw = fs::read_to_string(&args.runner_config_path)
        .map_err(|err| format!("failed to read runner config: {}", err))?;
    let mut runner: RunnerConfig = toml::from_str(&runner_raw)
        .map_err(|err| format!("failed to parse runner config: {}", err))?;

    if let Some(value) = args.run_count_override {
        runner.execution.run_count = value;
    }

    let scenario = load_scenario_with_secrets(&args.scenario_path, identity)?;
    validate_scenario_definition(&scenario)?;

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
        core: runner.core,
    })
}

fn validate_scenario_definition(scenario: &Scenario) -> Result<(), String> {
    if scenario.steps.is_empty() {
        return Err("scenario requires non-empty steps".to_string());
    }

    for (index, step) in scenario.steps.iter().enumerate() {
        match step {
            ScenarioStep::Conversation {
                tester_instructions,
                max_turns,
            } => {
                if tester_instructions.trim().is_empty() {
                    return Err(format!(
                        "steps[{}] conversation requires non-empty tester_instructions",
                        index
                    ));
                }
                if matches!(max_turns, Some(0)) {
                    return Err(format!(
                        "steps[{}] conversation max_turns must be >= 1 when specified",
                        index
                    ));
                }
            }
            ScenarioStep::EmitEvent { event, wait_for } => {
                if !event.kind.eq_ignore_ascii_case("trigger") {
                    return Err(format!(
                        "steps[{}] emit_event supports only event.type='trigger' currently",
                        index
                    ));
                }
                if let Some(spec) = wait_for {
                    if matches!(spec.timeout_ms, Some(0)) {
                        return Err(format!(
                            "steps[{}] emit_event wait_for.timeout_ms must be >= 1 when specified",
                            index
                        ));
                    }
                }
            }
        }
    }

    for key in REQUIRED_BASELINE_METRICS {
        if scenario
            .metrics_definition
            .get(key)
            .map(|metric| metric.exclude_from_pass)
            .unwrap_or(false)
        {
            return Err(format!(
                "metric '{}' cannot set exclude_from_pass=true",
                key
            ));
        }
    }

    Ok(())
}

fn ensure_openai_key() -> Result<(), String> {
    match std::env::var("OPENAI_API_KEY") {
        Ok(value) if !value.trim().is_empty() => Ok(()),
        _ => Err("OPENAI_API_KEY is required".to_string()),
    }
}

fn prepare_runtime(core: &CoreConfig) -> Result<RuntimeContext, String> {
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

    if let Some(prompts_file) = core.prompts_file.as_deref() {
        let prompts_path = resolve_to_manifest_path(PathBuf::from(prompts_file));
        if !prompts_path.exists() {
            return Err(format!(
                "core.prompts_file not found: {}",
                prompts_path.display()
            ));
        }

        let prompts_table = if let Some(table) = root.get_mut("prompts") {
            table
                .as_table_mut()
                .ok_or("config.toml [prompts] must be a table")?
        } else {
            root.as_table_mut()
                .ok_or("config.toml root must be a table")?
                .entry("prompts")
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
                .as_table_mut()
                .ok_or("failed to create [prompts] table")?
        };
        prompts_table.insert(
            "path".to_string(),
            toml::Value::String(prompts_path.to_string_lossy().to_string()),
        );
    }

    let patched =
        toml::to_string(&root).map_err(|err| format!("failed to render config: {}", err))?;
    let config_path = temp_dir.join("config.toml");
    fs::write(&config_path, patched)
        .map_err(|err| format!("failed to write temp config: {}", err))?;

    let auth_token =
        std::env::var("WEB_AUTH_TOKEN").unwrap_or_else(|_| DEFAULT_AUTH_TOKEN.to_string());
    let ws_url = format!("ws://127.0.0.1:{}/", port);
    let memgraph_uri = core.memgraph_uri.trim().to_string();
    if memgraph_uri.is_empty() {
        return Err("core.memgraph_uri must not be empty".to_string());
    }
    let memgraph_backup_path =
        resolve_to_manifest_path(PathBuf::from(core.memgraph_backup_path.as_str()));
    if !memgraph_backup_path.exists() {
        return Err(format!(
            "core.memgraph_backup_path not found: {}",
            memgraph_backup_path.display()
        ));
    }
    if !memgraph_backup_path.is_file() {
        return Err(format!(
            "core.memgraph_backup_path must be a file: {}",
            memgraph_backup_path.display()
        ));
    }

    Ok(RuntimeContext {
        temp_dir,
        db_path,
        ws_url,
        auth_token,
        memgraph_uri,
        memgraph_backup_path,
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
        .env("MEMGRAPH_URI", runtime.memgraph_uri.as_str())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| format!("failed to spawn core-rust: {}", err))
}

async fn restore_memgraph_snapshot(runtime: &RuntimeContext) -> Result<(), String> {
    let compose_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("compose.test.yaml");
    if !compose_file.exists() {
        return Err(format!(
            "compose.test.yaml not found: {}",
            compose_file.display()
        ));
    }
    let backup_name = runtime
        .memgraph_backup_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            format!(
                "invalid memgraph backup file name: {}",
                runtime.memgraph_backup_path.display()
            )
        })?
        .to_string();
    let backup_target = format!("memgraph-test:/data/snapshots/{}", backup_name);
    run_docker_compose(&compose_file, &["up", "-d", "memgraph-test"]).await?;
    run_docker_compose(
        &compose_file,
        &[
            "exec",
            "-u",
            "root",
            "memgraph-test",
            "mkdir",
            "-p",
            "/data/snapshots",
        ],
    )
    .await?;
    run_docker_compose(
        &compose_file,
        &[
            "cp",
            runtime.memgraph_backup_path.to_string_lossy().as_ref(),
            backup_target.as_str(),
        ],
    )
    .await?;
    run_docker_compose(
        &compose_file,
        &[
            "exec",
            "-u",
            "root",
            "memgraph-test",
            "chown",
            "memgraph:memgraph",
            format!("/data/snapshots/{}", backup_name).as_str(),
        ],
    )
    .await?;
    run_mgconsole_query(
        &compose_file,
        format!("RECOVER SNAPSHOT '/data/snapshots/{}' FORCE;", backup_name).as_str(),
    )
    .await?;
    for _ in 0..60 {
        if run_mgconsole_query(&compose_file, "RETURN 1;")
            .await
            .is_ok()
        {
            return Ok(());
        }
        sleep(Duration::from_millis(1_000)).await;
    }
    Err("memgraph-test did not become ready after snapshot restore".to_string())
}

async fn run_docker_compose(compose_file: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new("docker");
    command.arg("compose");
    command.arg("-f");
    command.arg(compose_file);
    for arg in args {
        command.arg(arg);
    }
    let output = command
        .output()
        .await
        .map_err(|err| format!("failed to run docker compose {:?}: {}", args, err))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "docker compose {:?} failed (status={}): {}",
        args,
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

async fn run_mgconsole_query(compose_file: &Path, query: &str) -> Result<(), String> {
    let mut command = Command::new("docker");
    command
        .arg("compose")
        .arg("-f")
        .arg(compose_file)
        .args(["exec", "-T", "memgraph-test", "mgconsole"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("failed to spawn mgconsole: {}", err))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("{}\n", query).as_bytes())
            .await
            .map_err(|err| format!("failed to write mgconsole stdin: {}", err))?;
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|err| format!("failed to wait mgconsole: {}", err))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "mgconsole query failed (status={}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
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
        let mut transcript: Vec<TranscriptTurn> = Vec::new();
        let mut response_times_ms: Vec<u64> = Vec::new();
        let mut raw_events: Vec<Value> = Vec::new();
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
                    transcript = value.transcript;
                    response_times_ms = value.response_times_ms;
                    raw_events = value.events;
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

        let log_file = write_event_log(
            assets.scenario.name.as_str(),
            run_index,
            &raw_events,
            assets.scenario.include_debug_events,
        )?;
        let log_file = log_file
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(log_file.as_path())
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string();
        let filtered_events = filter_events(raw_events, assets.scenario.include_debug_events);
        let (response_time_ms_mean, response_time_ms_min, response_time_ms_max) =
            response_time_stats(&response_times_ms);

        if let Some((code, detail)) = convo_error {
            let mut metrics = HashMap::new();
            let mut judge_summary = None;
            let mut failure_detail = detail;

            if !transcript.is_empty() || !filtered_events.is_empty() {
                match judge_run(assets, &transcript, &filtered_events).await {
                    Ok(output) => match validate_metrics(&assets.scenario, &output.metrics) {
                        Ok(()) => {
                            metrics = output.metrics;
                            judge_summary = output.summary;
                            if let Some(value) = metrics.get_mut("scenario_requirement_fit") {
                                *value = value.min(INCOMPLETE_SCENARIO_REQUIREMENT_CAP);
                            }
                        }
                        Err(err) => {
                            failure_detail =
                                format!("{} | partial_judge_invalid: {}", failure_detail, err);
                        }
                    },
                    Err(err) => {
                        failure_detail =
                            format!("{} | partial_judge_error: {}", failure_detail, err);
                    }
                }
            }

            runs.push(RunResult {
                run_index,
                pass: false,
                failure_code: Some(code),
                failure_detail: Some(failure_detail),
                metrics,
                judge_summary,
                turn_count: transcript.len(),
                event_count: filtered_events.len(),
                response_time_ms_mean,
                response_time_ms_min,
                response_time_ms_max,
                response_time_ms_by_turn: response_times_ms.clone(),
                message_log: transcript.clone(),
                log_file: Some(log_file),
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
                        response_time_ms_mean,
                        response_time_ms_min,
                        response_time_ms_max,
                        response_time_ms_by_turn: response_times_ms.clone(),
                        message_log: transcript.clone(),
                        log_file: Some(log_file.clone()),
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
                    response_time_ms_mean,
                    response_time_ms_min,
                    response_time_ms_max,
                    response_time_ms_by_turn: response_times_ms.clone(),
                    message_log: transcript.clone(),
                    log_file: Some(log_file.clone()),
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
                    response_time_ms_mean,
                    response_time_ms_min,
                    response_time_ms_max,
                    response_time_ms_by_turn: response_times_ms.clone(),
                    message_log: transcript.clone(),
                    log_file: Some(log_file.clone()),
                });
            }
        }
    }

    let metric_keys = collect_metric_keys(&assets.scenario);
    let pass_metric_keys = collect_pass_metric_keys(&assets.scenario);
    let gates = compute_gates(&runs, &metric_keys)?;
    let overall_pass = evaluate_overall_pass(&runs, &gates, &pass_metric_keys);

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
        memgraph_uri: runtime.memgraph_uri.clone(),
        memgraph_backup_path: runtime.memgraph_backup_path.to_string_lossy().to_string(),
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
            values.push(*run.metrics.get(key.as_str()).unwrap_or(&0.0));
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

fn collect_pass_metric_keys(scenario: &Scenario) -> Vec<String> {
    let mut keys = scenario
        .metrics_definition
        .iter()
        .filter_map(|(name, definition)| {
            if definition.exclude_from_pass {
                None
            } else {
                Some(name.clone())
            }
        })
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
) -> Result<DialogueRun, String> {
    let steps = build_runtime_scenario_steps(&assets.scenario, assets.execution.max_turns)?;
    let (mut ws_stream, _) = connect_async(runtime.ws_url.as_str())
        .await
        .map_err(|err| format!("websocket connect failed: {}", err))?;

    let auth = format!("{}:{}", DEFAULT_USER_NAME, runtime.auth_token);
    ws_stream
        .send(Message::Text(auth.into()))
        .await
        .map_err(|err| format!("auth send failed: {}", err))?;

    let mut transcript: Vec<TranscriptTurn> = Vec::new();
    let mut response_times_ms: Vec<u64> = Vec::new();
    let mut processed_reply_event_ids: HashSet<String> = HashSet::new();
    let mut processed_decision_event_ids: HashSet<String> = HashSet::new();
    let mut observed_event_ids: HashSet<String> = HashSet::new();
    let mut observed_events: Vec<Value> = Vec::new();
    let mut global_turn_index = 1_usize;

    for (step_index, step) in steps.iter().enumerate() {
        match step {
            RuntimeScenarioStep::Conversation(conversation) => {
                run_conversation_step(
                    assets,
                    &mut ws_stream,
                    conversation,
                    &mut transcript,
                    &mut response_times_ms,
                    &mut processed_reply_event_ids,
                    &mut processed_decision_event_ids,
                    &mut observed_event_ids,
                    &mut observed_events,
                    &mut global_turn_index,
                )
                .await?;
            }
            RuntimeScenarioStep::EmitEvent(emit) => {
                run_emit_event_step(
                    &mut ws_stream,
                    emit,
                    assets.execution.turn_timeout_ms,
                    step_index + 1,
                    &mut observed_event_ids,
                    &mut observed_events,
                )
                .await?;
            }
        }
    }

    let _ = ws_stream.send(Message::Close(None)).await;

    if transcript.is_empty() {
        return Err("tester produced no conversation turns".to_string());
    }

    Ok(DialogueRun {
        transcript,
        response_times_ms,
        events: observed_events,
    })
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn run_conversation_step(
    assets: &TestAssets,
    ws_stream: &mut WsStream,
    step: &ConversationRuntimeStep,
    transcript: &mut Vec<TranscriptTurn>,
    response_times_ms: &mut Vec<u64>,
    processed_reply_event_ids: &mut HashSet<String>,
    processed_decision_event_ids: &mut HashSet<String>,
    observed_event_ids: &mut HashSet<String>,
    observed_events: &mut Vec<Value>,
    global_turn_index: &mut usize,
) -> Result<(), String> {
    for _ in 0..step.max_turns {
        let utterance =
            generate_tester_utterance(assets, transcript, step.tester_instructions.as_str())
                .await?;
        if utterance == "__TEST_DONE__" {
            break;
        }
        if utterance.trim().is_empty() {
            return Err("tester returned empty utterance".to_string());
        }

        let turn_index = *global_turn_index;
        println!(
            "HARNESS_WS_SEND turn={} tester_text={}",
            turn_index,
            preview_text(&utterance, 200)
        );
        let payload = json!({
            "type": "message",
            "text": utterance,
        });
        let turn_started = Instant::now();
        ws_stream
            .send(Message::Text(payload.to_string().into()))
            .await
            .map_err(|err| format!("message send failed: {}", err))?;

        let turn_reply = wait_for_reply_text(
            ws_stream,
            assets.execution.turn_timeout_ms,
            processed_reply_event_ids,
            processed_decision_event_ids,
            observed_event_ids,
            observed_events,
            turn_index,
        )
        .await?;
        response_times_ms.push(turn_started.elapsed().as_millis() as u64);
        transcript.push(TranscriptTurn {
            user: payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            assistant: turn_reply.assistant,
        });
        if !turn_reply.saw_decision {
            wait_for_decision_event(
                ws_stream,
                assets.execution.turn_timeout_ms,
                processed_decision_event_ids,
                observed_event_ids,
                observed_events,
                turn_index,
            )
            .await?;
        }
        *global_turn_index += 1;
    }
    Ok(())
}

async fn run_emit_event_step(
    ws_stream: &mut WsStream,
    step: &EmitEventRuntimeStep,
    fallback_timeout_ms: u64,
    step_index: usize,
    observed_event_ids: &mut HashSet<String>,
    observed_events: &mut Vec<Value>,
) -> Result<(), String> {
    let payload = emit_event_payload_json(&step.event)?;
    println!("HARNESS_WS_SEND step={} emit_event={}", step_index, payload);
    ws_stream
        .send(Message::Text(payload.into()))
        .await
        .map_err(|err| format!("emit event send failed: {}", err))?;

    let timeout_ms = if step.wait_for_timeout_ms == 0 {
        fallback_timeout_ms
    } else {
        step.wait_for_timeout_ms
    };
    wait_for_emit_event_completion_ws(
        ws_stream,
        &step.wait_for_tags_any,
        timeout_ms,
        step_index,
        observed_event_ids,
        observed_events,
    )
    .await
}

fn build_runtime_scenario_steps(
    scenario: &Scenario,
    default_conversation_max_turns: usize,
) -> Result<Vec<RuntimeScenarioStep>, String> {
    if default_conversation_max_turns == 0 {
        return Err("execution.max_turns must be >= 1".to_string());
    }

    let mut out = Vec::with_capacity(scenario.steps.len());
    for (index, step) in scenario.steps.iter().enumerate() {
        match step {
            ScenarioStep::Conversation {
                tester_instructions,
                max_turns,
            } => {
                let resolved_max_turns = max_turns.unwrap_or(default_conversation_max_turns);
                if resolved_max_turns == 0 {
                    return Err(format!(
                        "steps[{}] conversation max_turns must be >= 1",
                        index
                    ));
                }
                out.push(RuntimeScenarioStep::Conversation(ConversationRuntimeStep {
                    tester_instructions: tester_instructions.clone(),
                    max_turns: resolved_max_turns,
                }));
            }
            ScenarioStep::EmitEvent { event, wait_for } => {
                let wait_for_tags_any = wait_for
                    .as_ref()
                    .map(|spec| {
                        if spec.tags_any.is_empty() {
                            DEFAULT_TRIGGER_WAIT_TAGS
                                .iter()
                                .map(|tag| tag.to_string())
                                .collect::<Vec<_>>()
                        } else {
                            spec.tags_any.clone()
                        }
                    })
                    .unwrap_or_else(|| {
                        DEFAULT_TRIGGER_WAIT_TAGS
                            .iter()
                            .map(|tag| tag.to_string())
                            .collect::<Vec<_>>()
                    });
                let wait_for_timeout_ms = wait_for
                    .as_ref()
                    .and_then(|spec| spec.timeout_ms)
                    .unwrap_or(DEFAULT_EMIT_WAIT_TIMEOUT_MS);
                out.push(RuntimeScenarioStep::EmitEvent(EmitEventRuntimeStep {
                    event: event.clone(),
                    wait_for_tags_any,
                    wait_for_timeout_ms,
                }));
            }
        }
    }
    Ok(out)
}

fn emit_event_payload_json(event: &EmitEventPayload) -> Result<String, String> {
    if !event.kind.eq_ignore_ascii_case("trigger") {
        return Err(format!(
            "unsupported emit_event type '{}': only 'trigger' is supported",
            event.kind
        ));
    }
    let target = event
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("all");
    let reason = event
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("integration scenario trigger");
    Ok(json!({
        "type": "trigger",
        "target": target,
        "reason": reason,
    })
    .to_string())
}

async fn wait_for_emit_event_completion_ws(
    ws_stream: &mut WsStream,
    tags_any: &[String],
    timeout_ms: u64,
    step_index: usize,
    observed_event_ids: &mut HashSet<String>,
    observed_events: &mut Vec<Value>,
) -> Result<(), String> {
    let effective_timeout_ms = timeout_ms.max(1);
    let deadline = Instant::now() + Duration::from_millis(effective_timeout_ms);
    let mut trigger_event_id: Option<String> = None;
    loop {
        if Instant::now() >= deadline {
            return Err(format!(
                "WAIT_FOR_EVENT_TIMEOUT step={} tags_any={} timeout_ms={}",
                step_index,
                tags_any.join(","),
                effective_timeout_ms
            ));
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = match timeout(remaining, ws_stream.next()).await {
            Ok(value) => value,
            Err(_) => {
                return Err(format!(
                    "WAIT_FOR_EVENT_TIMEOUT step={} tags_any={} timeout_ms={}",
                    step_index,
                    tags_any.join(","),
                    effective_timeout_ms
                ))
            }
        };

        match message {
            Some(Ok(Message::Text(text))) => {
                let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| Value::Null);
                maybe_record_runtime_event(&parsed, observed_event_ids, observed_events);

                if trigger_event_id.is_none()
                    && has_event_tag(&parsed, "self_improvement.triggered")
                {
                    trigger_event_id = extract_event_id(&parsed);
                }

                if event_matches_emit_completion(&parsed, tags_any, trigger_event_id.as_deref()) {
                    let matched_event_id =
                        extract_event_id(&parsed).unwrap_or_else(|| "-".to_string());
                    println!(
                        "HARNESS_EMIT_EVENT_WAIT_OK step={} matched_event_id={} trigger_event_id={} tags_any={}",
                        step_index,
                        matched_event_id,
                        trigger_event_id.as_deref().unwrap_or("-"),
                        tags_any.join(",")
                    );
                    return Ok(());
                }
            }
            Some(Ok(Message::Close(_))) => {
                return Err("websocket closed before emit_event completion".to_string());
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => return Err(format!("websocket receive failed: {}", err)),
            None => return Err("websocket stream ended before emit_event completion".to_string()),
        }
    }
}

fn response_time_stats(values: &[u64]) -> (Option<f64>, Option<u64>, Option<u64>) {
    if values.is_empty() {
        return (None, None, None);
    }
    let sum = values.iter().map(|value| *value as f64).sum::<f64>();
    let mean = round3(sum / values.len() as f64);
    let min = values.iter().copied().min();
    let max = values.iter().copied().max();
    (Some(mean), min, max)
}

async fn generate_tester_utterance(
    assets: &TestAssets,
    transcript: &[TranscriptTurn],
    tester_instructions: &str,
) -> Result<String, String> {
    let transcript_json = serde_json::to_string_pretty(transcript)
        .map_err(|err| format!("failed to serialize transcript: {}", err))?;
    let input = format!(
        "Scenario instructions:\n{}\n\nConversation transcript:\n{}\n\nIf scenario goals are already sufficiently exercised, output exactly __TEST_DONE__. Otherwise output exactly one next user utterance.",
        tester_instructions, transcript_json
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
    ws_stream: &mut WsStream,
    timeout_ms: u64,
    processed_reply_event_ids: &mut HashSet<String>,
    processed_decision_event_ids: &mut HashSet<String>,
    observed_event_ids: &mut HashSet<String>,
    observed_events: &mut Vec<Value>,
    turn_index: usize,
) -> Result<TurnReply, String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut saw_decision = false;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for reply after {}ms",
                timeout_ms
            ));
        }

        let remaining = deadline - now;
        let message = match timeout(remaining, ws_stream.next()).await {
            Ok(value) => value,
            Err(_) => {
                return Err(format!(
                    "timed out waiting for reply after {}ms",
                    timeout_ms
                ))
            }
        };

        match message {
            Some(Ok(Message::Text(text))) => {
                let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| Value::Null);
                maybe_record_runtime_event(&parsed, observed_event_ids, observed_events);
                let summary = extract_event_summary(&parsed);
                println!(
                    "HARNESS_WS_RECV turn={} event_id={} source={} tags={} reply_candidate={}",
                    turn_index,
                    summary
                        .as_ref()
                        .and_then(|value| value.event_id.as_deref())
                        .unwrap_or("-"),
                    summary
                        .as_ref()
                        .and_then(|value| value.source.as_deref())
                        .unwrap_or("-"),
                    summary
                        .as_ref()
                        .map(|value| value.tags.join(","))
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "-".to_string()),
                    summary
                        .as_ref()
                        .map(|value| value.is_reply)
                        .unwrap_or(false),
                );
                if is_decision_event(&parsed) {
                    if let Some(event_id) = extract_event_id(&parsed) {
                        if processed_decision_event_ids.insert(event_id.clone()) {
                            saw_decision = true;
                            println!(
                                "HARNESS_WS_DECISION_SEEN turn={} event_id={}",
                                turn_index, event_id
                            );
                        }
                    }
                }
                if is_reply_event(&parsed) {
                    let Some(event_id) = extract_event_id(&parsed) else {
                        println!(
                            "HARNESS_WS_REPLY_SKIP turn={} reason=missing_event_id",
                            turn_index
                        );
                        continue;
                    };
                    if processed_reply_event_ids.contains(&event_id) {
                        println!(
                            "HARNESS_WS_REPLY_SKIP turn={} reason=duplicate event_id={}",
                            turn_index, event_id
                        );
                        continue;
                    }
                    processed_reply_event_ids.insert(event_id);
                    let reply = extract_reply_text(&parsed);
                    println!(
                        "HARNESS_WS_REPLY_ACCEPT turn={} event_id={} assistant_text={}",
                        turn_index,
                        extract_event_id(&parsed).unwrap_or_else(|| "-".to_string()),
                        preview_text(&reply, 200)
                    );
                    return Ok(TurnReply {
                        assistant: reply,
                        saw_decision,
                    });
                }
            }
            Some(Ok(Message::Close(_))) => return Err("websocket closed before reply".to_string()),
            Some(Ok(_)) => {
                println!("HARNESS_WS_RECV turn={} non_text_message=true", turn_index);
            }
            Some(Err(err)) => return Err(format!("websocket receive failed: {}", err)),
            None => return Err("websocket stream ended before reply".to_string()),
        }
    }
}

async fn wait_for_decision_event(
    ws_stream: &mut WsStream,
    timeout_ms: u64,
    processed_decision_event_ids: &mut HashSet<String>,
    observed_event_ids: &mut HashSet<String>,
    observed_events: &mut Vec<Value>,
    turn_index: usize,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for decision after reply after {}ms",
                timeout_ms
            ));
        }

        let remaining = deadline - now;
        let message = match timeout(remaining, ws_stream.next()).await {
            Ok(value) => value,
            Err(_) => {
                return Err(format!(
                    "timed out waiting for decision after reply after {}ms",
                    timeout_ms
                ));
            }
        };

        match message {
            Some(Ok(Message::Text(text))) => {
                let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| Value::Null);
                maybe_record_runtime_event(&parsed, observed_event_ids, observed_events);
                let summary = extract_event_summary(&parsed);
                println!(
                    "HARNESS_WS_POST_REPLY_RECV turn={} event_id={} source={} tags={} decision_candidate={}",
                    turn_index,
                    summary
                        .as_ref()
                        .and_then(|value| value.event_id.as_deref())
                        .unwrap_or("-"),
                    summary
                        .as_ref()
                        .and_then(|value| value.source.as_deref())
                        .unwrap_or("-"),
                    summary
                        .as_ref()
                        .map(|value| value.tags.join(","))
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "-".to_string()),
                    is_decision_event(&parsed),
                );
                if is_decision_event(&parsed) {
                    let Some(event_id) = extract_event_id(&parsed) else {
                        continue;
                    };
                    if processed_decision_event_ids.insert(event_id.clone()) {
                        println!(
                            "HARNESS_WS_DECISION_ACCEPT turn={} event_id={}",
                            turn_index, event_id
                        );
                        return Ok(());
                    }
                }
            }
            Some(Ok(Message::Close(_))) => {
                return Err("websocket closed before decision completion".to_string());
            }
            Some(Ok(_)) => {
                println!(
                    "HARNESS_WS_POST_REPLY_RECV turn={} non_text_message=true",
                    turn_index
                );
            }
            Some(Err(err)) => return Err(format!("websocket receive failed: {}", err)),
            None => return Err("websocket stream ended before decision completion".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
struct EventSummary {
    event_id: Option<String>,
    source: Option<String>,
    tags: Vec<String>,
    is_reply: bool,
}

fn extract_event_summary(message: &Value) -> Option<EventSummary> {
    let obj = message.as_object()?;
    if obj.get("type").and_then(Value::as_str) != Some("event") {
        return None;
    }
    let event = obj.get("event").and_then(Value::as_object)?;
    let tags = event
        .get("meta")
        .and_then(|value| value.get("tags"))
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_str)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let is_reply = tags.iter().any(|tag| tag == "response");
    Some(EventSummary {
        event_id: event
            .get("event_id")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        source: event
            .get("source")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        tags,
        is_reply,
    })
}

fn extract_event_id(message: &Value) -> Option<String> {
    message
        .get("event")
        .and_then(Value::as_object)
        .and_then(|event| event.get("event_id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string())
}

fn has_event_tag(message: &Value, expected: &str) -> bool {
    message
        .get("event")
        .and_then(|value| value.get("meta"))
        .and_then(|value| value.get("tags"))
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .any(|tag| tag == expected)
        })
        .unwrap_or(false)
}

fn event_matches_emit_completion(
    message: &Value,
    tags_any: &[String],
    trigger_event_id: Option<&str>,
) -> bool {
    let event = match extract_runtime_event(message) {
        Some(value) => value,
        None => return false,
    };
    let tags = event
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let has_tag = tags
        .iter()
        .any(|tag| tags_any.iter().any(|expected| expected == tag));
    if !has_tag {
        return false;
    }

    let Some(trigger_id) = trigger_event_id else {
        return true;
    };
    event
        .get("payload")
        .and_then(|value| value.get("trigger_event_id"))
        .and_then(Value::as_str)
        .map(|value| value == trigger_id)
        .unwrap_or(false)
}

fn maybe_record_runtime_event(
    message: &Value,
    observed_event_ids: &mut HashSet<String>,
    observed_events: &mut Vec<Value>,
) {
    let Some(event) = extract_runtime_event(message) else {
        return;
    };
    let Some(event_id) = event
        .get("event_id")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    if !observed_event_ids.insert(event_id) {
        return;
    }
    observed_events.push(event);
}

fn extract_runtime_event(message: &Value) -> Option<Value> {
    let obj = message.as_object()?;
    if obj.get("type").and_then(Value::as_str) != Some("event") {
        return None;
    }
    let event = obj.get("event").and_then(Value::as_object)?;
    let tags = event
        .get("meta")
        .and_then(|value| value.get("tags"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(json!({
        "event_id": event.get("event_id").and_then(Value::as_str).unwrap_or(""),
        "ts": event.get("ts").and_then(Value::as_str).unwrap_or(""),
        "source": event.get("source").and_then(Value::as_str).unwrap_or(""),
        "modality": event.get("modality").and_then(Value::as_str).unwrap_or(""),
        "tags": tags,
        "payload": event.get("payload").cloned().unwrap_or(Value::Null),
    }))
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

    let mut has_response = false;
    for tag in tags.iter().filter_map(Value::as_str) {
        if tag == "response" {
            has_response = true;
        }
        if has_response {
            return true;
        }
    }
    false
}

fn is_decision_event(message: &Value) -> bool {
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
    if event.get("source").and_then(Value::as_str) != Some("decision") {
        return false;
    }
    event
        .get("meta")
        .and_then(|value| value.get("tags"))
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .any(|tag| tag == "decision")
        })
        .unwrap_or(false)
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
    let scenario_instruction_text = scenario_instructions_for_judge(&assets.scenario);
    let metrics_json = serde_json::to_string_pretty(&assets.scenario.metrics_definition)
        .map_err(|err| format!("failed to serialize metrics_definition: {}", err))?;
    let transcript_json = serde_json::to_string_pretty(transcript)
        .map_err(|err| format!("failed to serialize transcript: {}", err))?;
    let events_json = serde_json::to_string_pretty(events)
        .map_err(|err| format!("failed to serialize events: {}", err))?;

    let input = format!(
        "Scenario tester_instructions:\n{}\n\nMetrics definition:\n{}\n\nTranscript:\n{}\n\nEvents:\n{}\n\nReturn JSON only.",
        scenario_instruction_text, metrics_json, transcript_json, events_json
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

fn scenario_instructions_for_judge(scenario: &Scenario) -> String {
    let mut lines = Vec::<String>::new();
    lines.push("Scenario steps:".to_string());
    for (index, step) in scenario.steps.iter().enumerate() {
        match step {
            ScenarioStep::Conversation {
                tester_instructions,
                max_turns,
            } => {
                let turns_text = max_turns
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "runner_default".to_string());
                lines.push(format!(
                    "{}. conversation max_turns={} instructions={}",
                    index + 1,
                    turns_text,
                    tester_instructions.trim()
                ));
            }
            ScenarioStep::EmitEvent { event, wait_for } => {
                let target = event
                    .target
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("all");
                let reason = event
                    .reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("integration scenario trigger");
                let timeout_ms = wait_for
                    .as_ref()
                    .and_then(|spec| spec.timeout_ms)
                    .unwrap_or(DEFAULT_EMIT_WAIT_TIMEOUT_MS);
                let tags_text = wait_for
                    .as_ref()
                    .map(|spec| spec.tags_any.join(","))
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| DEFAULT_TRIGGER_WAIT_TAGS.join(","));
                lines.push(format!(
                    "{}. emit_event type={} target={} reason={} wait_for_tags_any={} wait_timeout_ms={}",
                    index + 1,
                    event.kind,
                    target,
                    reason,
                    tags_text,
                    timeout_ms
                ));
            }
        }
    }
    lines.join("\n")
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

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut preview = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if count >= max_chars {
            preview.push('…');
            break;
        }
        preview.push(ch);
        count += 1;
    }
    preview
}

fn write_event_log(
    scenario_name: &str,
    run_index: usize,
    events: &[Value],
    include_debug_events: bool,
) -> Result<PathBuf, String> {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/integration/logs");
    fs::create_dir_all(&base_dir).map_err(|err| format!("failed to create logs dir: {}", err))?;

    let ts_format = format_description::parse("[year][month][day]-[hour][minute][second]")
        .map_err(|err| format!("failed to parse time format: {}", err))?;
    let stamp = OffsetDateTime::now_utc()
        .format(&ts_format)
        .unwrap_or_else(|_| "log".to_string());
    let safe_name = scenario_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    let file_name = format!("{}__{}__run-{}.events.json", stamp, safe_name, run_index);
    let path = base_dir.join(file_name);

    let body = json!({
        "scenario_name": scenario_name,
        "run_index": run_index,
        "event_count": events.len(),
        "filter_mode": if include_debug_events { "include_debug" } else { "primary_only" },
        "events": events,
    });
    let text = serde_json::to_string_pretty(&body)
        .map_err(|err| format!("failed to serialize event log: {}", err))?;
    fs::write(&path, text).map_err(|err| format!("failed to write event log: {}", err))?;
    Ok(path)
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
