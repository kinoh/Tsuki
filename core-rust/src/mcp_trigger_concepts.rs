use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) const MAX_TRIGGER_CONCEPTS: usize = 3;

#[derive(Debug, Clone)]
pub(crate) struct TriggerConceptExtractionInput<'a> {
    pub(crate) server_id: &'a str,
    pub(crate) tool_name: &'a str,
    pub(crate) description: Option<&'a str>,
    pub(crate) input_schema: &'a Value,
}

#[derive(Debug, Deserialize)]
struct TriggerConceptsOutput {
    trigger_concepts: Vec<String>,
}

pub(crate) fn build_trigger_concept_prompts(
    input: &TriggerConceptExtractionInput<'_>,
) -> [String; 2] {
    let schema_text =
        serde_json::to_string(input.input_schema).unwrap_or_else(|_| "{}".to_string());
    let base_prompt = format!(
        "Extract trigger concepts for an MCP tool.\n\
Return strict JSON only with this shape: {{\"trigger_concepts\": [\"...\"]}}.\n\
No markdown. No explanation.\n\
Use natural language concepts directly (no prefixes).\n\
Return at most {max_trigger_concepts} trigger concepts.\n\
Prefer precision over recall.\n\
Choose generic action concepts that describe the tool category a user is explicitly asking for.\n\
Prefer stable action-family concepts over downstream use cases.\n\
Avoid concrete subcommands, protocols, domains, file formats, or task examples such as curl, jq, RSS, JSON, XML, URL, download, or news fetch.\n\
Avoid near-duplicates and paraphrase lists.\n\
If fewer than {max_trigger_concepts} concepts are justified, return fewer.\n\
\n\
server_id: {server_id}\n\
tool_name: {tool_name}\n\
description: {description}\n\
input_schema_json: {schema}",
        max_trigger_concepts = MAX_TRIGGER_CONCEPTS,
        server_id = input.server_id,
        tool_name = input.tool_name,
        description = input.description.unwrap_or("none"),
        schema = schema_text,
    );
    let retry_prompt = format!(
        "{base}\n\nIMPORTANT: Output exactly one JSON object. Example:\n{{\"trigger_concepts\":[\"run a command\",\"use the shell\",\"execute a terminal command\"]}}",
        base = base_prompt
    );
    [base_prompt, retry_prompt]
}

pub(crate) fn parse_trigger_concepts(raw: &str) -> Result<Vec<String>, String> {
    let parsed = serde_json::from_str::<TriggerConceptsOutput>(raw)
        .map_err(|err| format!("llm parse check failed: {}", err))?;
    let mut uniq = BTreeSet::<String>::new();
    for item in parsed.trigger_concepts {
        let normalized = item.trim();
        if normalized.is_empty() {
            continue;
        }
        uniq.insert(normalized.to_string());
    }
    let out = uniq
        .into_iter()
        .take(MAX_TRIGGER_CONCEPTS)
        .collect::<Vec<_>>();
    if out.is_empty() {
        return Err("llm non-empty check failed: no trigger concepts".to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_trigger_concepts_caps_count() {
        let raw = r#"{"trigger_concepts":["alpha","beta","gamma","delta"]}"#;
        let parsed = parse_trigger_concepts(raw).expect("should parse");
        assert_eq!(parsed, vec!["alpha", "beta", "delta"]);
        assert_eq!(parsed.len(), MAX_TRIGGER_CONCEPTS);
    }

    #[test]
    fn parse_trigger_concepts_dedupes_before_cap() {
        let raw = r#"{"trigger_concepts":["alpha","beta","alpha","gamma"]}"#;
        let parsed = parse_trigger_concepts(raw).expect("should parse");
        assert_eq!(parsed, vec!["alpha", "beta", "gamma"]);
    }
}
