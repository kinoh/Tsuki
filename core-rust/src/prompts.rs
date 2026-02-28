use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

#[derive(Clone, Default)]
pub struct PromptOverrides {
    pub base: Option<String>,
    pub router: Option<String>,
    pub decision: Option<String>,
    pub submodules: HashMap<String, String>,
}

pub fn load_prompts(path: &Path) -> Result<PromptOverrides, String> {
    if !path.exists() {
        return Err(format!("prompts file not found: {}", path.display()));
    }
    let raw = fs::read_to_string(path).map_err(|err| format!("failed to read prompts: {}", err))?;
    let prompts = parse_prompts(&raw)?;
    validate_required_core_sections(&prompts)?;
    validate_memory_sections(&prompts)?;
    Ok(prompts)
}

pub fn write_prompts(path: &Path, prompts: &PromptOverrides) -> Result<(), String> {
    validate_required_core_sections(prompts)?;
    validate_memory_sections(prompts)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create prompt dir: {}", err))?;
    }
    let mut output = String::new();
    output.push_str("# Base\n\n");
    output.push_str("```text\n");
    output.push_str(prompts.base.as_deref().unwrap_or(""));
    output.push_str("\n```\n\n");
    output.push_str("# Router\n\n");
    output.push_str("```text\n");
    output.push_str(prompts.router.as_deref().unwrap_or(""));
    output.push_str("\n```\n\n");
    output.push_str("# Decision\n\n");
    output.push_str("```text\n");
    output.push_str(prompts.decision.as_deref().unwrap_or(""));
    output.push_str("\n```\n\n");
    output.push_str("# Submodules\n\n");

    let mut ordered = BTreeMap::new();
    for (name, instructions) in prompts.submodules.iter() {
        ordered.insert(name, instructions);
    }
    for (name, instructions) in ordered {
        output.push_str(&format!("## {}\n\n", name));
        output.push_str("```text\n");
        output.push_str(instructions);
        output.push_str("\n```\n\n");
    }

    fs::write(path, output).map_err(|err| format!("failed to write prompts: {}", err))
}

fn parse_prompts(raw: &str) -> Result<PromptOverrides, String> {
    enum Section {
        None,
        Base,
        Router,
        Decision,
        Submodule(String),
    }

    let mut overrides = PromptOverrides::default();
    let mut section = Section::None;
    let mut in_block = false;
    let mut buffer: Vec<String> = Vec::new();
    let mut in_submodules = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if !in_block {
            if trimmed.starts_with("# ") {
                in_submodules = false;
                section = match trimmed {
                    "# Base" => Section::Base,
                    "# Router" => Section::Router,
                    "# Decision" => Section::Decision,
                    "# Submodules" => {
                        in_submodules = true;
                        Section::None
                    }
                    _ => Section::None,
                };
                continue;
            }
            if in_submodules && trimmed.starts_with("## ") {
                let name = trimmed.trim_start_matches("## ").trim().to_string();
                section = Section::Submodule(name);
                continue;
            }
            if trimmed == "```text" {
                in_block = true;
                buffer.clear();
            }
            continue;
        }

        if trimmed == "```" {
            let text = buffer.join("\n");
            match &section {
                Section::Base => overrides.base = Some(text),
                Section::Router => overrides.router = Some(text),
                Section::Decision => overrides.decision = Some(text),
                Section::Submodule(name) => {
                    overrides.submodules.insert(name.clone(), text);
                }
                Section::None => {}
            }
            in_block = false;
            buffer.clear();
            continue;
        }

        buffer.push(line.to_string());
    }

    Ok(overrides)
}

fn validate_required_core_sections(prompts: &PromptOverrides) -> Result<(), String> {
    let mut missing = Vec::<String>::new();
    if prompts
        .base
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing.push("Base".to_string());
    }
    if prompts
        .router
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing.push("Router".to_string());
    }
    if prompts
        .decision
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        missing.push("Decision".to_string());
    }
    if missing.is_empty() {
        return Ok(());
    }
    missing.sort();
    Err(format!(
        "prompts.md requires non-empty sections: {}",
        missing.join(", ")
    ))
}

fn validate_memory_sections(prompts: &PromptOverrides) -> Result<(), String> {
    let mut missing = Vec::<String>::new();
    if let Some(decision) = prompts.decision.as_deref() {
        if !has_markdown_h2_section(decision, "Memory") {
            missing.push("Decision".to_string());
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    missing.sort();
    Err(format!(
        "prompts.md requires `## Memory` section in: {}",
        missing.join(", ")
    ))
}

fn has_markdown_h2_section(source: &str, section_name: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with("## ") {
            return false;
        }
        let title = trimmed.trim_start_matches('#').trim();
        title == section_name
    })
}

#[cfg(test)]
mod tests {
    use super::{has_markdown_h2_section, validate_memory_sections, PromptOverrides};

    #[test]
    fn detects_memory_section() {
        let text = "hello\n## Memory\nvalue\n";
        assert!(has_markdown_h2_section(text, "Memory"));
    }

    #[test]
    fn validates_memory_sections_for_loaded_overrides() {
        let prompts = PromptOverrides {
            base: Some("base without memory section".to_string()),
            router: Some("router without memory section".to_string()),
            decision: Some("## Memory\ndecision".to_string()),
            submodules: [("curiosity".to_string(), "submodule without memory section".to_string())]
                .into_iter()
                .collect(),
        };
        assert!(validate_memory_sections(&prompts).is_ok());
    }

    #[test]
    fn rejects_loaded_overrides_without_decision_memory_section() {
        let prompts = PromptOverrides {
            base: Some("base without memory section".to_string()),
            router: Some("router without memory section".to_string()),
            decision: Some("decision without memory section".to_string()),
            submodules: [("curiosity".to_string(), "submodule without memory section".to_string())]
                .into_iter()
                .collect(),
        };
        let err = validate_memory_sections(&prompts).expect_err("must reject");
        assert!(err.contains("Decision"));
    }
}
