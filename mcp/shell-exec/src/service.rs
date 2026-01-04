use std::{env, process::Stdio, time::Duration};

use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, Implementation, ServerCapabilities, ServerInfo},
    schemars::{self, JsonSchema},
    serde_json::json,
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
};
use serde::Deserialize;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time,
};

const DEFAULT_MAX_OUTPUT_BYTES: usize = 40_000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteRequest {
    #[schemars(description = "Executable path or command name")]
    pub command: String,
    #[schemars(description = "Command arguments")]
    pub args: Option<Vec<String>>,
    #[schemars(description = "Stdin content to pass to the process")]
    pub stdin: Option<String>,
    #[schemars(description = "Optional timeout in milliseconds")]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone)]
pub struct ShellExecService {
    tool_router: ToolRouter<Self>,
    max_output_bytes: usize,
}

impl ShellExecService {
    pub fn from_env() -> Result<Self, ErrorData> {
        let max_output_bytes = match env::var("MCP_EXEC_MAX_OUTPUT_BYTES") {
            Ok(value) => value.parse::<usize>().map_err(|_| {
                ErrorData::invalid_params(
                    "Error: config: invalid MCP_EXEC_MAX_OUTPUT_BYTES",
                    Some(json!({"value": value})),
                )
            })?,
            Err(env::VarError::NotPresent) => DEFAULT_MAX_OUTPUT_BYTES,
            Err(err) => {
                return Err(ErrorData::invalid_params(
                    "Error: config: invalid MCP_EXEC_MAX_OUTPUT_BYTES",
                    Some(json!({"reason": err.to_string()})),
                ));
            }
        };

        Ok(Self {
            tool_router: Self::tool_router(),
            max_output_bytes,
        })
    }

    async fn execute_command(&self, request: ExecuteRequest) -> Result<CallToolResult, ErrorData> {
        if request.command.trim().is_empty() {
            return Err(ErrorData::invalid_params(
                "Error: command: empty",
                None,
            ));
        }

        let mut command = Command::new(&request.command);
        command
            .args(request.args.unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|err| {
            ErrorData::internal_error(
                "Error: execute: spawn failed",
                Some(json!({"reason": err.to_string()})),
            )
        })?;

        if let Some(stdin) = request.stdin {
            if let Some(mut child_stdin) = child.stdin.take() {
                child_stdin.write_all(stdin.as_bytes()).await.map_err(|err| {
                    ErrorData::internal_error(
                        "Error: execute: stdin write failed",
                        Some(json!({"reason": err.to_string()})),
                    )
                })?;
                child_stdin.shutdown().await.map_err(|err| {
                    ErrorData::internal_error(
                        "Error: execute: stdin close failed",
                        Some(json!({"reason": err.to_string()})),
                    )
                })?;
            }
        } else {
            drop(child.stdin.take());
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            ErrorData::internal_error("Error: execute: stdout unavailable", None)
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ErrorData::internal_error("Error: execute: stderr unavailable", None)
        })?;

        let max_output_bytes = self.max_output_bytes;
        let stdout_task = tokio::spawn(read_stream_limited(stdout, max_output_bytes));
        let stderr_task = tokio::spawn(read_stream_limited(stderr, max_output_bytes));

        let mut timed_out = false;
        let status = match request.timeout_ms {
            Some(timeout_ms) => match time::timeout(Duration::from_millis(timeout_ms), child.wait())
                .await
            {
                Ok(result) => result.map_err(|err| {
                    ErrorData::internal_error(
                        "Error: execute: wait failed",
                        Some(json!({"reason": err.to_string()})),
                    )
                })?,
                Err(_) => {
                    timed_out = true;
                    let _ = child.kill().await;
                    child.wait().await.map_err(|err| {
                        ErrorData::internal_error(
                            "Error: execute: wait failed",
                            Some(json!({"reason": err.to_string()})),
                        )
                    })?
                }
            },
            None => child.wait().await.map_err(|err| {
                ErrorData::internal_error(
                    "Error: execute: wait failed",
                    Some(json!({"reason": err.to_string()})),
                )
            })?,
        };

        let (mut stdout_bytes, stdout_truncated) = stdout_task
            .await
            .map_err(|err| {
                ErrorData::internal_error(
                    "Error: execute: stdout task failed",
                    Some(json!({"reason": err.to_string()})),
                )
            })??;
        let (mut stderr_bytes, stderr_truncated) = stderr_task
            .await
            .map_err(|err| {
                ErrorData::internal_error(
                    "Error: execute: stderr task failed",
                    Some(json!({"reason": err.to_string()})),
                )
            })??;

        let mut output_truncated = stdout_truncated || stderr_truncated;
        if stdout_bytes.len() + stderr_bytes.len() > max_output_bytes {
            let allowed = max_output_bytes.saturating_sub(stdout_bytes.len());
            if stderr_bytes.len() > allowed {
                stderr_bytes.truncate(allowed);
                output_truncated = true;
            }
        }

        let result = json!({
            "stdout": String::from_utf8_lossy(&stdout_bytes).to_string(),
            "stderr": String::from_utf8_lossy(&stderr_bytes).to_string(),
            "exit_code": status.code(),
            "timed_out": timed_out,
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated,
            "output_truncated": output_truncated,
        });

        Ok(CallToolResult {
            content: vec![Content::text(result.to_string())],
            structured_content: Some(result),
            is_error: Some(false),
            meta: None,
        })
    }
}

#[tool_router]
impl ShellExecService {
    #[tool(description = "Executes a shell command inside the sandbox")]
    pub async fn execute(
        &self,
        Parameters(request): Parameters<ExecuteRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        self.execute_command(request).await
    }
}

#[tool_handler]
impl ServerHandler for ShellExecService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Shell command MCP server for running commands inside a sandbox container".into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: env!("CARGO_CRATE_NAME").to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

async fn read_stream_limited<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> Result<(Vec<u8>, bool), ErrorData> {
    let mut buffer = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];

    loop {
        let read = reader.read(&mut chunk).await.map_err(|err| {
            ErrorData::internal_error(
                "Error: execute: read failed",
                Some(json!({"reason": err.to_string()})),
            )
        })?;
        if read == 0 {
            break;
        }

        if buffer.len() < max_bytes {
            let remaining = max_bytes - buffer.len();
            let take = read.min(remaining);
            buffer.extend_from_slice(&chunk[..take]);
            if read > remaining {
                truncated = true;
            }
        } else {
            truncated = true;
        }
    }

    Ok((buffer, truncated))
}
