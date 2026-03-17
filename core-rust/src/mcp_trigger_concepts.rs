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
    prompt_template: &str,
    retry_prompt_template: &str,
) -> [String; 2] {
    let schema_text =
        serde_json::to_string(input.input_schema).unwrap_or_else(|_| "{}".to_string());
    let base_prompt = prompt_template
        .replace(
            "{{max_trigger_concepts}}",
            &MAX_TRIGGER_CONCEPTS.to_string(),
        )
        .replace("{{server_id}}", input.server_id)
        .replace("{{tool_name}}", input.tool_name)
        .replace("{{description}}", input.description.unwrap_or("none"))
        .replace("{{input_schema_json}}", &schema_text);
    let retry_prompt = retry_prompt_template.replace("{{base_prompt}}", &base_prompt);
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
