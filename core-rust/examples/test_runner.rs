use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;
use url::Url;

const DEFAULT_WS_URL: &str = "ws://localhost:2953/";
const TIMEOUT_MS: u64 = 60_000;
const INTERVAL_MS: u64 = 500;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = RunnerArgs::parse()?;
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let ws_url = std::env::var("WS_URL").unwrap_or_else(|_| DEFAULT_WS_URL.to_string());

    let mut core_proc = if args.connect_only {
        None
    } else {
        Some(start_core(manifest_dir).await?)
    };

    wait_for_ws(&ws_url, &mut core_proc).await?;

    let status = Command::new("cargo")
        .args([
            "run",
            "--example",
            "ws_scenario",
            "--",
            args.scenario_path.to_string_lossy().as_ref(),
        ])
        .current_dir(manifest_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|err| format!("failed to run ws_scenario: {}", err))?;

    if let Some(mut child) = core_proc {
        let _ = child.kill().await;
    }

    if !status.success() {
        return Err("ws_scenario exited with a non-zero status".to_string());
    }

    Ok(())
}

struct RunnerArgs {
    connect_only: bool,
    scenario_path: std::path::PathBuf,
}

impl RunnerArgs {
    fn parse() -> Result<Self, String> {
        let mut connect_only = false;
        let mut scenario_path: Option<std::path::PathBuf> = None;

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--connect" {
                connect_only = true;
                continue;
            }
            if arg == "--help" || arg == "-h" {
                return Err(usage());
            }
            if arg.starts_with('-') {
                return Err(format!("unknown flag: {}", arg));
            }
            if scenario_path.is_some() {
                return Err(usage());
            }
            scenario_path = Some(std::path::PathBuf::from(arg));
        }

        let scenario_path = scenario_path.ok_or_else(usage)?;
        Ok(Self {
            connect_only,
            scenario_path,
        })
    }
}

fn usage() -> String {
    "Usage: cargo run --example test_runner -- [--connect] <scenario.yaml>".to_string()
}

async fn start_core(manifest_dir: &str) -> Result<tokio::process::Child, String> {
    Command::new("cargo")
        .args(["run", "--bin", "tsuki-core-rust"])
        .current_dir(manifest_dir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| format!("failed to start core: {}", err))
}

async fn wait_for_ws(
    ws_url: &str,
    core_proc: &mut Option<tokio::process::Child>,
) -> Result<(), String> {
    let url = Url::parse(ws_url).map_err(|err| format!("invalid WS_URL: {}", err))?;
    let host = url.host_str().ok_or("WS_URL missing host")?.to_string();
    let port = match (url.port(), url.scheme()) {
        (Some(port), _) => port,
        (None, "wss") => 443,
        (None, _) => 80,
    };

    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < TIMEOUT_MS as u128 {
        if let Some(child) = core_proc.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("core exited early: {}", status));
            }
        }

        if try_connect(&host, port).await {
            return Ok(());
        }
        sleep(Duration::from_millis(INTERVAL_MS)).await;
    }

    Err(format!("timed out waiting for WS: {}", ws_url))
}

async fn try_connect(host: &str, port: u16) -> bool {
    let addr = format!("{}:{}", host, port);
    matches!(
        tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}
