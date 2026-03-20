#[allow(dead_code)]
#[path = "../activation_concept_graph.rs"]
mod activation_concept_graph;
#[allow(dead_code)]
#[path = "../conversation_recall_store.rs"]
mod conversation_recall_store;
#[allow(dead_code)]
#[path = "../input_ingress.rs"]
mod input_ingress;
#[allow(dead_code)]
#[path = "../llm.rs"]
mod llm;
#[allow(dead_code)]
#[path = "../multimodal_embedding.rs"]
mod multimodal_embedding;

#[allow(dead_code)]
mod event {
    use serde_json::Value;

    #[derive(Debug, Clone)]
    pub struct Event {
        pub(crate) event_id: String,
        pub(crate) ts: String,
        pub(crate) source: String,
        pub(crate) modality: String,
        pub(crate) payload: Value,
        pub(crate) meta: EventMeta,
    }

    #[derive(Debug, Clone)]
    pub struct EventMeta {
        pub(crate) tags: Vec<String>,
    }

    pub(crate) fn rehydrate_event(
        event_id: String,
        ts: String,
        source: String,
        modality: String,
        payload: Value,
        tags: Vec<String>,
    ) -> Event {
        Event {
            event_id,
            ts,
            source,
            modality,
            payload,
            meta: EventMeta { tags },
        }
    }

    pub(crate) mod contracts {
        use serde_json::json;

        use super::{Event, EventMeta};

        pub(crate) fn input_text(source: &str, kind: &str, text: &str) -> Event {
            Event {
                event_id: "stub-input".to_string(),
                ts: "2026-03-09T00:00:00Z".to_string(),
                source: source.to_string(),
                modality: "text".to_string(),
                payload: json!({ "text": text }),
                meta: EventMeta {
                    tags: vec!["input".to_string(), format!("type:{}", kind)],
                },
            }
        }

        pub(crate) fn response_text(text: String) -> Event {
            Event {
                event_id: "stub-response".to_string(),
                ts: "2026-03-09T00:00:00Z".to_string(),
                source: "assistant".to_string(),
                modality: "text".to_string(),
                payload: json!({ "text": text }),
                meta: EventMeta {
                    tags: vec!["response".to_string()],
                },
            }
        }
    }
}

use activation_concept_graph::{
    ActivationConceptGraphStore, ConceptGraphDebugReader, ConceptGraphOps,
};
use llm::{build_response_api_llm, LlmRequest, ResponseApiConfig};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
struct Cli {
    config_path: PathBuf,
    concepts_file: Option<PathBuf>,
    submodules: Vec<String>,
    include_all: bool,
    limit: usize,
    max_selected: usize,
    apply: bool,
    model: Option<String>,
    relation_type: String,
    output_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RootConfig {
    llm: LlmConfig,
    internal_prompts: InternalPromptConfig,
    #[serde(default)]
    modules: Vec<ModuleConfig>,
}

#[derive(Debug, Deserialize)]
struct LlmConfig {
    model: String,
}

#[derive(Debug, Deserialize)]
struct InternalPromptConfig {
    concept_link_selection_instructions: String,
    concept_link_selection_prompt_template: String,
}

#[derive(Debug, Deserialize)]
struct ModuleConfig {
    name: String,
    instructions: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct SelectionOutput {
    #[serde(default)]
    selected: Vec<String>,
}

#[derive(Debug)]
struct SubmoduleSelection {
    submodule: String,
    selected: Vec<String>,
}

#[derive(Debug)]
struct ApplyStats {
    submodule_concept: String,
    selected_count: usize,
    relation_ok: usize,
    relation_failed: usize,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("ERROR: {}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let cli = parse_cli()?;
    let config_text = fs::read_to_string(&cli.config_path)
        .map_err(|err| format!("config read error: {}", err))?;
    let config = toml::from_str::<RootConfig>(&config_text)
        .map_err(|err| format!("config parse error: {}", err))?;

    let enabled_modules = config
        .modules
        .iter()
        .filter(|m| m.enabled)
        .map(|m| (m.name.clone(), m.instructions.clone()))
        .collect::<HashMap<_, _>>();
    if enabled_modules.is_empty() {
        return Err("no enabled modules found in config".to_string());
    }
    let targets = resolve_targets(&cli, &enabled_modules)?;
    if targets.is_empty() {
        return Err("no target submodules resolved".to_string());
    }

    let model = cli
        .model
        .clone()
        .or_else(|| env::var("OPENAI_MODEL").ok())
        .unwrap_or(config.llm.model);
    let store = connect_store().await?;
    let concepts = load_candidate_concepts(&store, &cli).await?;
    if concepts.is_empty() {
        return Err("candidate concepts are empty".to_string());
    }

    println!(
        "LINK_SUBMODULE_CONCEPTS_START mode={} targets={} candidates={} model={} relation_type={}",
        if cli.apply { "apply" } else { "dry-run" },
        targets.join(","),
        concepts.len(),
        model,
        cli.relation_type
    );

    let mut selections = Vec::<SubmoduleSelection>::new();
    for submodule in &targets {
        let instructions = enabled_modules
            .get(submodule)
            .ok_or_else(|| format!("module instructions not found: {}", submodule))?;
        let selected = select_concepts_for_submodule(
            submodule,
            instructions,
            &concepts,
            cli.max_selected,
            &model,
            &config.internal_prompts,
        )
        .await?;
        println!(
            "SUBMODULE_SELECTION submodule={} selected={}",
            submodule,
            selected.len()
        );
        selections.push(SubmoduleSelection {
            submodule: submodule.clone(),
            selected,
        });
    }

    let mut apply_stats = Vec::<ApplyStats>::new();
    if cli.apply {
        for item in &selections {
            let submodule_concept = format!("submodule:{}", item.submodule);
            store
                .concept_upsert(submodule_concept.clone())
                .await
                .map_err(|err| {
                    format!("concept_upsert failed for {}: {}", submodule_concept, err)
                })?;
            let mut relation_ok = 0usize;
            let mut relation_failed = 0usize;
            for concept in &item.selected {
                let result = store
                    .relation_add(
                        concept.clone(),
                        submodule_concept.clone(),
                        cli.relation_type.clone(),
                    )
                    .await;
                if result.is_ok() {
                    relation_ok += 1;
                } else {
                    relation_failed += 1;
                    eprintln!(
                        "RELATION_ADD_FAILED submodule={} concept={} error={}",
                        item.submodule,
                        concept,
                        result.err().unwrap_or_else(|| "unknown error".to_string())
                    );
                }
            }
            apply_stats.push(ApplyStats {
                submodule_concept,
                selected_count: item.selected.len(),
                relation_ok,
                relation_failed,
            });
        }
    }

    let output = build_output(&cli, &targets, &concepts, &selections, &apply_stats);
    if let Some(path) = &cli.output_path {
        let rendered =
            serde_json::to_string_pretty(&output).map_err(|err| format!("json error: {}", err))?;
        fs::write(path, rendered).map_err(|err| format!("failed to write output: {}", err))?;
        println!("WROTE_OUTPUT {}", path.display());
    }

    println!(
        "LINK_SUBMODULE_CONCEPTS_DONE mode={} submodules={} applied_relations={}",
        if cli.apply { "apply" } else { "dry-run" },
        selections.len(),
        apply_stats.iter().map(|s| s.relation_ok).sum::<usize>()
    );
    Ok(())
}

fn parse_cli() -> Result<Cli, String> {
    let mut config_path = PathBuf::from("config.toml");
    let mut concepts_file = None::<PathBuf>;
    let mut submodules = Vec::<String>::new();
    let mut include_all = false;
    let mut limit = 200usize;
    let mut max_selected = 40usize;
    let mut apply = false;
    let mut model = None::<String>;
    let mut relation_type = "evokes".to_string();
    let mut output_path = None::<PathBuf>;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --config".to_string())?;
                config_path = PathBuf::from(value);
            }
            "--concepts-file" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --concepts-file".to_string())?;
                concepts_file = Some(PathBuf::from(value));
            }
            "--submodule" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --submodule".to_string())?;
                submodules.push(value);
            }
            "--all" => {
                include_all = true;
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --limit".to_string())?;
                limit = value
                    .parse::<usize>()
                    .map_err(|err| format!("invalid --limit: {}", err))?;
            }
            "--max-selected" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --max-selected".to_string())?;
                max_selected = value
                    .parse::<usize>()
                    .map_err(|err| format!("invalid --max-selected: {}", err))?;
            }
            "--apply" => {
                apply = true;
            }
            "--model" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --model".to_string())?;
                model = Some(value);
            }
            "--relation-type" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --relation-type".to_string())?;
                relation_type = value;
            }
            "--output" => {
                let value = args
                    .next()
                    .ok_or_else(|| "missing value for --output".to_string())?;
                output_path = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                return Err(format!("unknown argument: {}", arg));
            }
        }
    }

    let relation_type_lc = relation_type.to_ascii_lowercase();
    if !matches!(relation_type_lc.as_str(), "evokes" | "is-a" | "part-of") {
        return Err("relation_type must be one of: evokes|is-a|part-of".to_string());
    }
    if limit == 0 {
        return Err("--limit must be >= 1".to_string());
    }
    if max_selected == 0 {
        return Err("--max-selected must be >= 1".to_string());
    }

    Ok(Cli {
        config_path,
        concepts_file,
        submodules,
        include_all,
        limit,
        max_selected,
        apply,
        model,
        relation_type: relation_type_lc,
        output_path,
    })
}

fn print_usage() {
    println!(
        "Usage: cargo run --bin link_submodule_concepts -- [--config config.toml] [--all | --submodule <name> ...] [--concepts-file path] [--limit N] [--max-selected N] [--model NAME] [--relation-type evokes|is-a|part-of] [--apply] [--output path]"
    );
}

fn resolve_targets(
    cli: &Cli,
    enabled_modules: &HashMap<String, String>,
) -> Result<Vec<String>, String> {
    if cli.include_all || cli.submodules.is_empty() {
        let mut all = enabled_modules.keys().cloned().collect::<Vec<_>>();
        all.sort();
        return Ok(all);
    }

    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for submodule in &cli.submodules {
        if !enabled_modules.contains_key(submodule) {
            return Err(format!(
                "submodule '{}' is not enabled or not defined in config",
                submodule
            ));
        }
        if seen.insert(submodule.clone()) {
            out.push(submodule.clone());
        }
    }
    Ok(out)
}

async fn connect_store() -> Result<ActivationConceptGraphStore, String> {
    let arousal_tau_ms = env::var("AROUSAL_TAU_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(86_400_000.0);
    ActivationConceptGraphStore::connect(
        env::var("MEMGRAPH_URI").unwrap_or_else(|_| "bolt://localhost:7687".to_string()),
        env::var("MEMGRAPH_USER").unwrap_or_default(),
        env::var("MEMGRAPH_PASSWORD").unwrap_or_default(),
        arousal_tau_ms,
        None,
    )
    .await
}

async fn load_candidate_concepts(
    store: &ActivationConceptGraphStore,
    cli: &Cli,
) -> Result<Vec<String>, String> {
    let raw = if let Some(path) = &cli.concepts_file {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("failed to read concepts file {}: {}", path.display(), err))?;
        parse_concepts_text(&text)?
    } else {
        let rows = store.debug_concept_search(None, cli.limit).await?;
        rows.into_iter()
            .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
            .collect::<Vec<_>>()
    };

    let mut set = BTreeSet::<String>::new();
    for concept in raw {
        let name = concept.trim();
        if name.is_empty() {
            continue;
        }
        if name.starts_with("submodule:") {
            continue;
        }
        set.insert(name.to_string());
    }
    Ok(set.into_iter().collect::<Vec<_>>())
}

fn parse_concepts_text(text: &str) -> Result<Vec<String>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.starts_with('[') {
        let values = serde_json::from_str::<Vec<String>>(trimmed)
            .map_err(|err| format!("concepts-file json array parse error: {}", err))?;
        return Ok(values);
    }
    Ok(trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>())
}

async fn select_concepts_for_submodule(
    submodule: &str,
    instructions: &str,
    concepts: &[String],
    max_selected: usize,
    model: &str,
    internal_prompts: &InternalPromptConfig,
) -> Result<Vec<String>, String> {
    let concept_lines = concepts
        .iter()
        .map(|name| format!("- {}", name))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = internal_prompts
        .concept_link_selection_prompt_template
        .replace("{{submodule}}", submodule)
        .replace("{{instructions}}", instructions)
        .replace("{{concept_lines}}", &concept_lines)
        .replace("{{max_selected}}", &max_selected.to_string());

    let adapter = build_response_api_llm(ResponseApiConfig {
        model: model.to_string(),
        instructions: internal_prompts.concept_link_selection_instructions.clone(),
        temperature: None,
        max_output_tokens: Some(4_000),
        tools: Vec::new(),
        tool_handler: None,
        usage_recorder: None,
        usage_context: None,
        max_tool_rounds: 0,
    });
    let response = adapter
        .respond(LlmRequest { input: prompt })
        .await
        .map_err(|err| err.to_string())?;
    let parsed = parse_selection_output(response.text.as_str())?;
    let allow = concepts.iter().cloned().collect::<HashSet<_>>();
    let mut selected = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for item in parsed.selected {
        let name = item.trim();
        if name.is_empty() {
            continue;
        }
        if !allow.contains(name) {
            continue;
        }
        if seen.insert(name.to_string()) {
            selected.push(name.to_string());
        }
        if selected.len() >= max_selected {
            break;
        }
    }
    Ok(selected)
}

fn parse_selection_output(raw: &str) -> Result<SelectionOutput, String> {
    if let Ok(value) = serde_json::from_str::<SelectionOutput>(raw) {
        return Ok(value);
    }
    let start = raw
        .find('{')
        .ok_or_else(|| "selection output does not include JSON object".to_string())?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| "selection output does not include JSON object".to_string())?;
    let candidate = raw
        .get(start..=end)
        .ok_or_else(|| "failed to slice selection JSON object".to_string())?;
    serde_json::from_str::<SelectionOutput>(candidate)
        .map_err(|err| format!("selection json parse error: {}", err))
}

fn build_output(
    cli: &Cli,
    targets: &[String],
    concepts: &[String],
    selections: &[SubmoduleSelection],
    apply_stats: &[ApplyStats],
) -> Value {
    json!({
        "mode": if cli.apply { "apply" } else { "dry-run" },
        "relation_type": cli.relation_type,
        "targets": targets,
        "candidate_concepts_count": concepts.len(),
        "candidate_concepts": concepts,
        "selections": selections.iter().map(|item| {
            json!({
                "submodule": item.submodule,
                "selected_count": item.selected.len(),
                "selected": item.selected,
            })
        }).collect::<Vec<_>>(),
        "apply_stats": apply_stats.iter().map(|item| {
            json!({
                "submodule_concept": item.submodule_concept,
                "selected_count": item.selected_count,
                "relation_ok": item.relation_ok,
                "relation_failed": item.relation_failed,
            })
        }).collect::<Vec<_>>(),
    })
}
