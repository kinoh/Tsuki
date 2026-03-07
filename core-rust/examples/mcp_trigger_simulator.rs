#[path = "../src/llm.rs"]
mod llm;
#[path = "../src/mcp_trigger_concepts.rs"]
mod mcp_trigger_concepts;

use llm::{build_response_api_llm, LlmAdapter, LlmRequest, ResponseApiConfig};
use mcp_trigger_concepts::{
    build_trigger_concept_prompts, parse_trigger_concepts, TriggerConceptExtractionInput,
};
use rmcp::model::{ClientCapabilities, ClientInfo, Implementation};
use rmcp::service::ServiceExt;
use rmcp::transport::StreamableHttpClientTransport;
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone)]
struct Args {
    url: String,
    tool_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolObject {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    #[serde(alias = "inputSchema")]
    input_schema: Option<Value>,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("ERROR: {}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    let tools = discover_tools(args.url.as_str()).await?;

    if let Some(tool_name) = args.tool_name.as_deref() {
        let tool = tools
            .iter()
            .find(|item| item.name == tool_name)
            .ok_or_else(|| format!("tool not found: {}", tool_name))?;
        let server_id = derive_server_id(args.url.as_str());
        simulate_trigger_concepts(args.url.as_str(), server_id.as_str(), tool).await
    } else {
        print_tool_list(args.url.as_str(), &tools);
        Ok(())
    }
}

fn parse_args() -> Result<Args, String> {
    let mut url = None;
    let mut tool_name = None;
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--url" => {
                let value = it.next().ok_or("--url requires a value")?;
                url = Some(value);
            }
            "--tool" => {
                let value = it.next().ok_or("--tool requires a value")?;
                tool_name = Some(value);
            }
            "--help" | "-h" => {
                return Err(
                    "Usage: cargo run --example mcp_trigger_simulator -- --url <mcp-url> [--tool <tool-name>]"
                        .to_string(),
                );
            }
            other => {
                return Err(format!("unknown argument: {}", other));
            }
        }
    }
    Ok(Args {
        url: url.ok_or("--url is required")?,
        tool_name,
    })
}

async fn discover_tools(url: &str) -> Result<Vec<ToolObject>, String> {
    let transport = StreamableHttpClientTransport::from_uri(url.to_string());
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "tsuki-core-rust".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ..Default::default()
        },
    };
    let client = client_info
        .serve(transport)
        .await
        .map_err(|err| format!("mcp client init failed: {}", err))?;
    let response = client
        .list_tools(None)
        .await
        .map_err(|err| format!("mcp tools/list failed: {}", err))?;
    let tools = response
        .tools
        .iter()
        .map(|item| {
            serde_json::to_value(item)
                .ok()
                .and_then(|value| serde_json::from_value::<ToolObject>(value).ok())
                .unwrap_or_else(|| ToolObject {
                    name: item.name.to_string(),
                    description: None,
                    input_schema: None,
                })
        })
        .collect::<Vec<_>>();
    let _ = client.cancel().await;
    Ok(tools)
}

fn print_tool_list(url: &str, tools: &[ToolObject]) {
    println!("url: {}", url);
    println!("tools:");
    for tool in tools {
        println!("- {}", tool.name);
        println!(
            "  description: {}",
            tool.description.as_deref().unwrap_or("none")
        );
    }
}

fn derive_server_id(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|value| value.host_str().map(str::to_string))
        .unwrap_or_else(|| "mcp_server".to_string())
}

async fn simulate_trigger_concepts(
    url: &str,
    server_id: &str,
    tool: &ToolObject,
) -> Result<(), String> {
    let llm = build_llm();
    let input_schema = tool.input_schema.clone().unwrap_or_else(|| json!({}));
    let prompts = build_trigger_concept_prompts(&TriggerConceptExtractionInput {
        server_id,
        tool_name: tool.name.as_str(),
        description: tool.description.as_deref(),
        input_schema: &input_schema,
    });
    let mut last_error = "llm parse check failed: empty output".to_string();
    let mut last_raw = Value::Null;
    let mut last_output = String::new();
    let mut last_prompt = String::new();

    for prompt in prompts {
        let response = llm
            .respond(LlmRequest {
                input: prompt.clone(),
            })
            .await
            .map_err(|err| format!("llm call failed: {}", err))?;
        last_raw = response.raw.clone();
        last_output = response.text.clone();
        last_prompt = prompt.clone();
        let candidates = collect_output_candidates(&response.text, &response.raw);
        if candidates.is_empty() {
            last_error = "llm parse check failed: empty output".to_string();
            continue;
        }
        for candidate in candidates {
            match parse_trigger_concepts(candidate.as_str()) {
                Ok(trigger_concepts) => {
                    print_trigger_result(
                        url,
                        server_id,
                        tool,
                        &input_schema,
                        prompt.as_str(),
                        response.text.as_str(),
                        &trigger_concepts,
                    );
                    return Ok(());
                }
                Err(err) => {
                    last_error = err;
                }
            }
        }
    }

    print_failed_trigger_result(
        url,
        server_id,
        tool,
        &input_schema,
        last_prompt.as_str(),
        last_output.as_str(),
        &last_raw,
        last_error.as_str(),
    );
    Err(last_error)
}

fn build_llm() -> Arc<dyn LlmAdapter> {
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-5.2".to_string());
    build_response_api_llm(ResponseApiConfig {
        model,
        instructions: "Extract trigger concepts for MCP tools. Return strict JSON only."
            .to_string(),
        temperature: None,
        max_output_tokens: Some(200),
        tools: Vec::new(),
        tool_handler: None,
        usage_recorder: None,
        usage_context: None,
        max_tool_rounds: 0,
    })
}

fn collect_output_candidates(text: &str, raw: &Value) -> Vec<String> {
    let mut out = Vec::<String>::new();
    if let Some(value) = normalize_output_candidate(text) {
        out.push(value);
    }
    if let Some(value) = extract_output_text_from_raw_json(raw) {
        if !out.iter().any(|item| item == &value) {
            out.push(value);
        }
    }
    out
}

fn normalize_output_candidate(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "(empty response)" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn extract_output_text_from_raw_json(value: &Value) -> Option<String> {
    let output = value.get("output")?.as_array()?;
    for item in output {
        let content = item.get("content").and_then(Value::as_array);
        let Some(content) = content else {
            continue;
        };
        for chunk in content {
            if let Some(text) = chunk.get("text").and_then(Value::as_str) {
                if let Some(candidate) = normalize_output_candidate(text) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn print_trigger_result(
    url: &str,
    server_id: &str,
    tool: &ToolObject,
    input_schema: &Value,
    prompt: &str,
    raw_output_text: &str,
    trigger_concepts: &[String],
) {
    println!("url: {}", url);
    println!("server_id: {}", server_id);
    println!("tool: {}", tool.name);
    println!();
    println!("description:");
    println!("{}", tool.description.as_deref().unwrap_or("none"));
    println!();
    println!("input_schema_json:");
    println!(
        "{}",
        serde_json::to_string_pretty(input_schema).unwrap_or_else(|_| "{}".to_string())
    );
    println!();
    println!("prompt:");
    println!("{}", prompt);
    println!();
    println!("raw_llm_output:");
    println!("{}", raw_output_text);
    println!();
    println!("normalized_trigger_concepts:");
    for concept in trigger_concepts {
        println!("- {}", concept);
    }
}

fn print_failed_trigger_result(
    url: &str,
    server_id: &str,
    tool: &ToolObject,
    input_schema: &Value,
    prompt: &str,
    raw_output_text: &str,
    raw_response: &Value,
    error: &str,
) {
    println!("url: {}", url);
    println!("server_id: {}", server_id);
    println!("tool: {}", tool.name);
    println!();
    println!("description:");
    println!("{}", tool.description.as_deref().unwrap_or("none"));
    println!();
    println!("input_schema_json:");
    println!(
        "{}",
        serde_json::to_string_pretty(input_schema).unwrap_or_else(|_| "{}".to_string())
    );
    println!();
    println!("prompt:");
    println!("{}", prompt);
    println!();
    println!("raw_llm_output:");
    println!("{}", raw_output_text);
    println!();
    println!("raw_llm_response_json:");
    println!(
        "{}",
        serde_json::to_string_pretty(raw_response).unwrap_or_else(|_| "null".to_string())
    );
    println!();
    println!("parse_error:");
    println!("{}", error);
}
