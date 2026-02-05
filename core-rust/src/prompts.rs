use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

#[derive(Clone, Default)]
pub struct PromptOverrides {
  pub base: Option<String>,
  pub decision: Option<String>,
  pub submodules: HashMap<String, String>,
}

pub const DEFAULT_PROMPTS_PATH: &str = "data/prompts.md";

pub fn load_prompts(path: &Path) -> Result<PromptOverrides, String> {
  if !path.exists() {
    return Ok(PromptOverrides::default());
  }
  let raw = fs::read_to_string(path).map_err(|err| format!("failed to read prompts: {}", err))?;
  parse_prompts(&raw)
}

pub fn write_prompts(path: &Path, prompts: &PromptOverrides) -> Result<(), String> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)
      .map_err(|err| format!("failed to create prompt dir: {}", err))?;
  }
  let mut output = String::new();
  output.push_str("# Base\n\n");
  output.push_str("```text\n");
  output.push_str(prompts.base.as_deref().unwrap_or(""));
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
