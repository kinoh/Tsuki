use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

use crate::app_state::AppState;
use crate::llm::{build_response_api_llm, LlmRequest, ResponseApiConfig};

const MAX_TRIGGER_CONCEPTS: usize = 3;
const DEFAULT_LIST_LIMIT: usize = 80;
const MAX_LIST_LIMIT: usize = 200;
const SKILL_INSTALL_TOOL: &str = "shell_exec__skill_install";
const SKILL_READ_TOOL: &str = "shell_exec__skill_read";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct SkillInstallFile {
    pub(crate) path: String,
    pub(crate) body: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillCatalogItem {
    pub(crate) key: String,
    pub(crate) summary: String,
    pub(crate) body_state_key: String,
    pub(crate) required_mcp_tools: Vec<String>,
    pub(crate) disabled: bool,
    pub(crate) valence: f64,
    pub(crate) arousal: f64,
    pub(crate) accessed_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillPackageDetail {
    pub(crate) content: String,
    pub(crate) files: Vec<SkillInstallFile>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillAdminDetail {
    pub(crate) key: String,
    pub(crate) installed: bool,
    pub(crate) concept: Option<Value>,
    pub(crate) summary: String,
    pub(crate) body_state_key: String,
    pub(crate) required_mcp_tools: Vec<String>,
    pub(crate) trigger_concepts: Vec<String>,
    pub(crate) package: Option<SkillPackageDetail>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SkillUpsertPayload {
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) trigger_concepts: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) files: Vec<SkillInstallFile>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillUpsertResult {
    pub(crate) key: String,
    pub(crate) summary: String,
    pub(crate) trigger_concepts: Vec<String>,
    pub(crate) required_mcp_tools: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GeneratedSkillIndex {
    summary: String,
    #[serde(default)]
    trigger_concepts: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    #[serde(default)]
    required_mcp_tools: Vec<String>,
}

pub(crate) async fn upsert_skill(
    state: &AppState,
    key: &str,
    payload: SkillUpsertPayload,
) -> Result<SkillUpsertResult, (StatusCode, String)> {
    let SkillUpsertPayload {
        content,
        summary,
        trigger_concepts,
        files,
    } = payload;
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "key is required".to_string()));
    }
    if content.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "content is required".to_string()));
    }
    let required_mcp_tools = parse_required_mcp_tools(&content, state)?;
    let install_files = build_skill_install_files(&content, &files)?;

    // 1. Install files in sandbox.
    state
        .services
        .mcp_registry
        .call_tool(
            SKILL_INSTALL_TOOL,
            json!({
                "key": key,
                "files": install_files
            }),
        )
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("skill_install failed: {}", err),
            )
        })?;

    // 2. Resolve summary and trigger_concepts (explicit or LLM-generated).
    let (summary, trigger_concepts) =
        resolve_skill_index(state, &key, &content, summary, trigger_concepts).await?;

    let skill_name = format!("skill:{}", key);

    // 3. Update concept graph.
    state
        .services
        .activation_concept_graph
        .skill_index_upsert(
            skill_name.clone(),
            summary.clone(),
            key.clone(),
            required_mcp_tools.clone(),
            true,
        )
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    state
        .services
        .activation_concept_graph
        .skill_index_replace_triggers(skill_name, trigger_concepts.clone())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;

    Ok(SkillUpsertResult {
        key,
        summary,
        trigger_concepts,
        required_mcp_tools,
    })
}

pub(crate) async fn list_skills(
    state: &AppState,
    query: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<SkillCatalogItem>, (StatusCode, String)> {
    let limit = limit
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .max(1)
        .min(MAX_LIST_LIMIT);
    let query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with("skill:") {
                value.to_string()
            } else {
                format!("skill:{}", value)
            }
        })
        .or_else(|| Some("skill:".to_string()));
    let rows = state
        .services
        .activation_concept_graph
        .debug_concept_search(query, limit)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    let mut items = Vec::<SkillCatalogItem>::new();
    for row in rows {
        let Some(name) = row.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        if row.get("kind").and_then(|value| value.as_str()) != Some("skill") {
            continue;
        }
        let key = name
            .strip_prefix("skill:")
            .unwrap_or(name)
            .trim()
            .to_string();
        if key.is_empty() {
            continue;
        }
        let summary = row
            .get("summary")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let body_state_key = row
            .get("body_state_key")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let required_mcp_tools = row
            .get("required_mcp_tools")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(str::trim))
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let disabled = row
            .get("disabled")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let valence = row
            .get("valence")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        let arousal = row
            .get("arousal")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0);
        let accessed_at = row
            .get("accessed_at")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        items.push(SkillCatalogItem {
            key,
            summary,
            body_state_key,
            required_mcp_tools,
            disabled,
            valence,
            arousal,
            accessed_at,
        });
    }
    Ok(items)
}

pub(crate) async fn get_skill_detail(
    state: &AppState,
    key: &str,
) -> Result<Option<SkillAdminDetail>, (StatusCode, String)> {
    let key = key
        .trim()
        .strip_prefix("skill:")
        .unwrap_or(key.trim())
        .trim();
    if key.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "key is required".to_string()));
    }

    let concept_name = format!("skill:{}", key);
    let concept = state
        .services
        .activation_concept_graph
        .debug_concept_detail(concept_name.clone())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    let package = read_skill_package(state, key).await?;

    if concept.is_none() && package.is_none() {
        return Ok(None);
    }

    let (summary, body_state_key, required_mcp_tools, trigger_concepts) = concept
        .as_ref()
        .map(extract_skill_metadata)
        .unwrap_or_else(|| {
            (
                String::new(),
                key.to_string(),
                Vec::<String>::new(),
                Vec::<String>::new(),
            )
        });

    Ok(Some(SkillAdminDetail {
        key: key.to_string(),
        installed: package.is_some(),
        concept,
        summary,
        body_state_key,
        required_mcp_tools,
        trigger_concepts,
        package,
    }))
}

fn build_skill_install_files(
    content: &str,
    auxiliary_files: &[SkillInstallFile],
) -> Result<Vec<serde_json::Value>, (StatusCode, String)> {
    let mut files = vec![json!({
        "path": "SKILL.md",
        "body": content,
    })];
    let mut seen_paths = BTreeSet::<String>::new();
    for file in auxiliary_files {
        let path = file.path.trim();
        if path.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "files contains empty path".to_string(),
            ));
        }
        if path == "SKILL.md" {
            return Err((
                StatusCode::BAD_REQUEST,
                "files must not include SKILL.md; use content for the skill body".to_string(),
            ));
        }
        if path.contains("..") || path.starts_with('/') {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("files contains invalid path: {}", path),
            ));
        }
        if !seen_paths.insert(path.to_string()) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("files contains duplicate path: {}", path),
            ));
        }
        files.push(json!({
            "path": path,
            "body": file.body,
        }));
    }
    Ok(files)
}

fn parse_required_mcp_tools(
    content: &str,
    state: &AppState,
) -> Result<Vec<String>, (StatusCode, String)> {
    let Some(frontmatter) = extract_skill_frontmatter(content).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid skill frontmatter: {}", err),
        )
    })?
    else {
        return Ok(Vec::new());
    };
    let parsed = serde_yaml::from_str::<SkillFrontmatter>(&frontmatter).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid skill frontmatter: {}", err),
        )
    })?;
    normalize_required_mcp_tools(parsed.required_mcp_tools, state)
}

fn extract_skill_frontmatter(content: &str) -> Result<Option<String>, String> {
    let mut lines = content.lines();
    let Some(first_line) = lines.next() else {
        return Ok(None);
    };
    if first_line.trim() != "---" {
        return Ok(None);
    }

    let mut yaml_lines = Vec::<&str>::new();
    for line in lines {
        if line.trim() == "---" {
            return Ok(Some(yaml_lines.join("\n")));
        }
        yaml_lines.push(line);
    }
    Err("missing closing ---".to_string())
}

fn normalize_required_mcp_tools(
    items: Vec<String>,
    state: &AppState,
) -> Result<Vec<String>, (StatusCode, String)> {
    let mut deduped = Vec::<String>::new();
    let mut seen = BTreeSet::<String>::new();
    for item in items {
        let value = item.trim();
        if value.is_empty() {
            continue;
        }
        if !state.services.mcp_registry.contains_runtime_tool(value) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "required_mcp_tools contains unknown MCP runtime tool: {}",
                    value
                ),
            ));
        }
        if seen.insert(value.to_string()) {
            deduped.push(value.to_string());
        }
    }
    Ok(deduped)
}

async fn resolve_skill_index(
    state: &AppState,
    key: &str,
    content: &str,
    summary: Option<String>,
    trigger_concepts: Option<Vec<String>>,
) -> Result<(String, Vec<String>), (StatusCode, String)> {
    match (
        summary.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        trigger_concepts.as_deref(),
    ) {
        (Some(_), None) | (None, Some(_)) => Err((
            StatusCode::BAD_REQUEST,
            "summary and trigger_concepts must be provided together".to_string(),
        )),
        (Some(s), Some(tc)) => {
            let concepts = normalize_trigger_concepts(tc)?;
            Ok((s.to_string(), concepts))
        }
        (None, None) => generate_skill_index(state, key, content).await,
    }
}

fn normalize_trigger_concepts(items: &[String]) -> Result<Vec<String>, (StatusCode, String)> {
    let mut deduped = Vec::<String>::new();
    for item in items {
        let value = item.trim();
        if value.is_empty() || deduped.iter().any(|existing| existing == value) {
            continue;
        }
        if deduped.len() >= MAX_TRIGGER_CONCEPTS {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "trigger_concepts must contain at most {} unique items",
                    MAX_TRIGGER_CONCEPTS
                ),
            ));
        }
        deduped.push(value.to_string());
    }
    if deduped.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "trigger_concepts must contain at least one non-empty item".to_string(),
        ));
    }
    Ok(deduped)
}

#[derive(Debug, Deserialize)]
struct SkillReadResult {
    found: bool,
    #[serde(default)]
    content: String,
    #[serde(default)]
    files: Vec<String>,
}

async fn read_skill_package(
    state: &AppState,
    key: &str,
) -> Result<Option<SkillPackageDetail>, (StatusCode, String)> {
    let root = read_skill_file(state, key, None).await?;
    let Some(root) = root else {
        return Ok(None);
    };

    let mut files = Vec::<SkillInstallFile>::new();
    for path in root.files.into_iter().filter(|path| path != "SKILL.md") {
        let Some(file) = read_skill_file(state, key, Some(path.as_str())).await? else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("skill_read missing file: {}", path),
            ));
        };
        files.push(SkillInstallFile {
            path,
            body: file.content,
        });
    }

    Ok(Some(SkillPackageDetail {
        content: root.content,
        files,
    }))
}

async fn read_skill_file(
    state: &AppState,
    key: &str,
    path: Option<&str>,
) -> Result<Option<SkillReadResult>, (StatusCode, String)> {
    let mut payload = json!({ "key": key });
    if let Some(path) = path {
        payload["path"] = json!(path);
    }
    let raw = state
        .services
        .mcp_registry
        .call_tool(SKILL_READ_TOOL, payload)
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("skill_read failed: {}", err),
            )
        })?;
    let parsed = serde_json::from_str::<SkillReadResult>(raw.as_str()).map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("skill_read parse failed: {}", err),
        )
    })?;
    if !parsed.found {
        return Ok(None);
    }
    Ok(Some(parsed))
}

fn extract_skill_metadata(concept: &Value) -> (String, String, Vec<String>, Vec<String>) {
    let summary = concept
        .get("summary")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let body_state_key = concept
        .get("body_state_key")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let required_mcp_tools = concept
        .get("required_mcp_tools")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_str().map(str::trim))
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let trigger_concepts = concept
        .get("relations")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let direction = item.get("direction").and_then(|value| value.as_str())?;
                    let relation_type = item.get("type").and_then(|value| value.as_str())?;
                    let from = item.get("from").and_then(|value| value.as_str())?;
                    let to = item.get("to").and_then(|value| value.as_str())?;
                    if direction != "incoming"
                        || relation_type != "evokes"
                        || !to.starts_with("skill:")
                    {
                        return None;
                    }
                    let trigger = from.strip_prefix("skill:").unwrap_or(from).trim();
                    if trigger.is_empty() {
                        None
                    } else {
                        Some(trigger.to_string())
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    (
        summary,
        body_state_key,
        required_mcp_tools,
        dedupe_strings(trigger_concepts),
    )
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::<String>::new();
    let mut deduped = Vec::<String>::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            deduped.push(trimmed.to_string());
        }
    }
    deduped
}

async fn generate_skill_index(
    state: &AppState,
    key: &str,
    body: &str,
) -> Result<(String, Vec<String>), (StatusCode, String)> {
    let adapter = build_response_api_llm(ResponseApiConfig {
        model: state.runtime.modules.runtime.model.clone(),
        instructions: state
            .config
            .internal_prompts
            .skill_index_instructions
            .clone(),
        temperature: state.runtime.modules.runtime.temperature,
        max_output_tokens: Some(500),
        tools: Vec::new(),
        tool_handler: None,
        usage_recorder: None,
        usage_context: None,
        max_tool_rounds: 0,
    });
    let prompt = state
        .config
        .internal_prompts
        .skill_index_prompt_template
        .replace(
            "{{max_trigger_concepts}}",
            &MAX_TRIGGER_CONCEPTS.to_string(),
        )
        .replace("{{skill_key}}", key)
        .replace("{{skill_body}}", body);
    let response = adapter
        .respond(LlmRequest { input: prompt })
        .await
        .map_err(|err| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("skill index generation failed: {}", err),
            )
        })?;
    let parsed =
        parse_generated(&response.text).map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok((parsed.summary, parsed.trigger_concepts))
}

fn parse_generated(raw: &str) -> Result<GeneratedSkillIndex, String> {
    let text = raw.trim();
    let parsed = serde_json::from_str::<GeneratedSkillIndex>(text)
        .or_else(|_| {
            let start = text
                .find('{')
                .ok_or_else(|| "missing json object".to_string())?;
            let end = text
                .rfind('}')
                .ok_or_else(|| "missing json object end".to_string())?;
            if end < start {
                return Err("invalid json range".to_string());
            }
            serde_json::from_str::<GeneratedSkillIndex>(&text[start..=end])
                .map_err(|err| err.to_string())
        })
        .map_err(|err| format!("skill index parse failed: {}", err))?;
    let summary = parsed.summary.trim().to_string();
    if summary.is_empty() {
        return Err("skill index parse failed: empty summary".to_string());
    }
    let mut deduped = Vec::<String>::new();
    for item in parsed.trigger_concepts {
        let value = item.trim();
        if value.is_empty() || deduped.iter().any(|existing| existing == value) {
            continue;
        }
        deduped.push(value.to_string());
        if deduped.len() >= MAX_TRIGGER_CONCEPTS {
            break;
        }
    }
    Ok(GeneratedSkillIndex {
        summary,
        trigger_concepts: deduped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_skill_frontmatter_returns_none_without_header() {
        let frontmatter =
            extract_skill_frontmatter("# Skill\nbody").expect("frontmatter parse should succeed");
        assert!(frontmatter.is_none());
    }

    #[test]
    fn extract_skill_frontmatter_returns_yaml_block() {
        let frontmatter = extract_skill_frontmatter(
            "---\nname: web_page_extract\nrequired_mcp_tools:\n  - shell_exec__execute\n---\n# Skill\nbody",
        )
        .expect("frontmatter parse should succeed")
        .expect("frontmatter should exist");
        let parsed: SkillFrontmatter =
            serde_yaml::from_str(&frontmatter).expect("yaml should parse");
        assert_eq!(
            parsed.required_mcp_tools,
            vec!["shell_exec__execute".to_string()]
        );
    }

    #[test]
    fn extract_skill_frontmatter_rejects_unclosed_block() {
        let error =
            extract_skill_frontmatter("---\nrequired_mcp_tools:\n  - shell_exec__execute\n# Skill")
                .expect_err("unclosed frontmatter should fail");
        assert!(error.contains("missing closing"));
    }

    #[test]
    fn build_skill_install_files_includes_auxiliary_files() {
        let files = build_skill_install_files(
            "# Skill\nbody",
            &[SkillInstallFile {
                path: "scripts/fetch.js".to_string(),
                body: "console.log('ok');".to_string(),
            }],
        )
        .expect("file payload should be valid");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["path"], "SKILL.md");
        assert_eq!(files[1]["path"], "scripts/fetch.js");
    }

    #[test]
    fn build_skill_install_files_rejects_skill_md_auxiliary_file() {
        let error = build_skill_install_files(
            "# Skill\nbody",
            &[SkillInstallFile {
                path: "SKILL.md".to_string(),
                body: "duplicate".to_string(),
            }],
        )
        .expect_err("SKILL.md auxiliary file should be rejected");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert!(error.1.contains("must not include SKILL.md"));
    }

    #[test]
    fn extract_skill_metadata_reads_required_tools_and_triggers() {
        let concept = json!({
            "summary": "Extract news page text",
            "body_state_key": "web_page_extract",
            "required_mcp_tools": ["shell_exec__execute"],
            "relations": [
                {
                    "direction": "incoming",
                    "from": "news_fetch",
                    "to": "skill:web_page_extract",
                    "type": "evokes",
                    "weight": 0.4
                },
                {
                    "direction": "outgoing",
                    "from": "skill:web_page_extract",
                    "to": "article",
                    "type": "evokes",
                    "weight": 0.2
                }
            ]
        });

        let (summary, body_state_key, required_mcp_tools, trigger_concepts) =
            extract_skill_metadata(&concept);
        assert_eq!(summary, "Extract news page text");
        assert_eq!(body_state_key, "web_page_extract");
        assert_eq!(required_mcp_tools, vec!["shell_exec__execute".to_string()]);
        assert_eq!(trigger_concepts, vec!["news_fetch".to_string()]);
    }
}
