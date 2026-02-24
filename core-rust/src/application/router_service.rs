use futures::future::join_all;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    future::Future,
    time::Instant,
};

use crate::event::build_event;
use crate::llm::{LlmAdapter, LlmRequest, ResponseApiAdapter, ResponseApiConfig, ToolError};
use crate::prompts::PromptOverrides;
use crate::{record_event, AppState, ModuleRuntime, Modules};

const ROUTER_RECALL_MAX_HOP: u32 = 2;
const SATURATION_STEP: f64 = 0.24;
const SATURATION_MAX: f64 = 0.72;
const SATURATION_RECOVERY: f64 = 0.06;
const POST_HARD_DAMPEN_RATIO: f64 = 0.35;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HardTriggerResult {
    pub(crate) module: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RouterOutput {
    pub(crate) activation_query_terms: Vec<String>,
    pub(crate) active_concepts_from_concept_graph: String,
    pub(crate) module_scores: BTreeMap<String, f64>,
    pub(crate) saturation_penalties: BTreeMap<String, f64>,
    pub(crate) hard_effective_scores: BTreeMap<String, f64>,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
    pub(crate) hard_trigger_results: Vec<HardTriggerResult>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActivationSnapshot {
    pub(crate) active_concepts_from_concept_graph: String,
    pub(crate) hard_triggers: Vec<String>,
    pub(crate) soft_recommendations: Vec<String>,
}

#[derive(Debug, Clone)]
struct RouterPreprocessOutput {
    query_terms: Vec<String>,
    candidate_concepts: Vec<String>,
}

#[derive(Debug, Clone)]
struct RouterConceptResolution {
    selected_seeds: Vec<String>,
    active_concepts_from_concept_graph: String,
}

pub(crate) async fn run_router<F, Fut>(
    input_text: &str,
    module_instructions: &HashMap<String, String>,
    modules: &Modules,
    state: &AppState,
    overrides: &PromptOverrides,
    execute_submodule: F,
) -> RouterOutput
where
    F: Fn(&str, &ActivationSnapshot, &str, Option<&str>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolError>> + Send,
{
    let router_started = Instant::now();
    let active_module_names = module_instructions.keys().cloned().collect::<Vec<_>>();
    let concept_limit = state.router.query_terms_max.max(1);
    let preprocess = preprocess_router_activation(input_text, concept_limit, state).await;
    let resolution = resolve_active_concepts_from_concept_graph(
        input_text,
        &active_module_names,
        &preprocess,
        concept_limit,
        modules,
        state,
        overrides,
    )
    .await;
    let activation_query_terms = preprocess.query_terms.clone();
    emit_concept_graph_query_event(
        state,
        &preprocess.query_terms,
        concept_limit,
        &preprocess.candidate_concepts,
        &resolution.selected_seeds,
        &resolution.active_concepts_from_concept_graph,
    )
    .await;
    let activation_sources = collect_submodule_activation_sources(&resolution.selected_seeds);
    if !activation_sources.is_empty() {
        if let Err(err) = state
            .activation_concept_graph
            .activate_related_submodules(activation_sources)
            .await
        {
            emit_router_debug_error(state, "activate_related_submodules", &err).await;
        }
    }

    let scores = compute_module_scores_from_concept_activation(&active_module_names, state).await;
    let module_scores = scores
        .iter()
        .map(|(name, score)| (name.clone(), *score))
        .collect::<BTreeMap<_, _>>();
    let saturation_levels = read_saturation_levels(state, &active_module_names).await;
    let (hard_scores, saturation_penalties, hard_effective_scores) =
        apply_saturation_penalty(&scores, &saturation_levels);
    let hard_triggers =
        select_modules_by_threshold(&hard_scores, state.router.hard_trigger_threshold);
    let soft_recommendations =
        select_modules_by_threshold(&scores, state.router.recommendation_threshold);
    let activation_snapshot = ActivationSnapshot {
        active_concepts_from_concept_graph: resolution.active_concepts_from_concept_graph.clone(),
        hard_triggers: hard_triggers.clone(),
        soft_recommendations: soft_recommendations.clone(),
    };
    let hard_trigger_results = run_hard_triggers(
        &activation_snapshot,
        module_instructions,
        &execute_submodule,
    )
    .await;
    update_saturation_levels(state, &active_module_names, &hard_triggers).await;
    dampen_hard_triggered_submodule_arousal(state, &hard_triggers).await;

    let router_output = RouterOutput {
        activation_query_terms,
        active_concepts_from_concept_graph: resolution.active_concepts_from_concept_graph,
        module_scores,
        saturation_penalties,
        hard_effective_scores,
        hard_triggers,
        soft_recommendations,
        hard_trigger_results,
    };
    let router_event = build_event(
        "router",
        "state",
        serde_json::to_value(&router_output)
            .unwrap_or_else(|_| json!({ "error": "router_output_serialize_failed" })),
        vec!["router".to_string()],
    );
    record_event(state, router_event).await;
    println!(
        "PERF router stage=end total_ms={} active_modules={} hard_triggers={} hard_results={} soft_recommendations={} concepts_len={}",
        router_started.elapsed().as_millis(),
        active_module_names.len(),
        router_output.hard_triggers.len(),
        router_output.hard_trigger_results.len(),
        router_output.soft_recommendations.len(),
        router_output.active_concepts_from_concept_graph.len()
    );
    router_output
}

pub(crate) fn activation_snapshot_from_router_output(
    router_output: &RouterOutput,
) -> ActivationSnapshot {
    ActivationSnapshot {
        active_concepts_from_concept_graph: router_output
            .active_concepts_from_concept_graph
            .clone(),
        hard_triggers: router_output.hard_triggers.clone(),
        soft_recommendations: router_output.soft_recommendations.clone(),
    }
}

async fn run_hard_triggers<F, Fut>(
    activation_snapshot: &ActivationSnapshot,
    module_instructions: &HashMap<String, String>,
    execute_submodule: &F,
) -> Vec<HardTriggerResult>
where
    F: Fn(&str, &ActivationSnapshot, &str, Option<&str>) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolError>> + Send,
{
    if activation_snapshot.hard_triggers.is_empty() {
        return Vec::new();
    }
    let started = Instant::now();
    let runs = activation_snapshot
        .hard_triggers
        .iter()
        .filter_map(|module_name| {
            module_instructions
                .get(module_name)
                .map(|instructions| (module_name.clone(), instructions.clone()))
        })
        .map(|(module_name, instructions)| async move {
            let module_started = Instant::now();
            let result = execute_submodule(
                &module_name,
                activation_snapshot,
                &instructions,
                Some("hard_trigger"),
            )
            .await;
            println!(
                "PERF router.hard_trigger module={} ms={} ok={}",
                module_name,
                module_started.elapsed().as_millis(),
                result.is_ok()
            );
            (module_name, result)
        })
        .collect::<Vec<_>>();
    let outputs = join_all(runs)
        .await
        .into_iter()
        .map(|(module, result)| match result {
            Ok(text) => HardTriggerResult { module, text },
            Err(err) => HardTriggerResult {
                module,
                text: format!("error: {}", err),
            },
        })
        .collect::<Vec<_>>();
    println!(
        "PERF router.hard_trigger stage=end total_ms={} count={}",
        started.elapsed().as_millis(),
        outputs.len()
    );
    outputs
}

fn render_router_context_template(
    template: &str,
    latest_user_input: &str,
    active_submodules: &str,
    query_terms: &str,
    candidate_concepts: &str,
    router_query_terms_max: usize,
) -> String {
    template
        .replace("{{latest_user_input}}", latest_user_input)
        .replace("{{active_submodules}}", active_submodules)
        .replace("{{query_terms}}", query_terms)
        .replace("{{candidate_concepts}}", candidate_concepts)
        .replace(
            "{{router_query_terms_max}}",
            &router_query_terms_max.to_string(),
        )
}

async fn compute_module_scores_from_concept_activation(
    active_module_names: &[String],
    state: &AppState,
) -> Vec<(String, f64)> {
    let submodule_concepts = active_module_names
        .iter()
        .map(|name| format!("submodule:{}", name))
        .collect::<Vec<_>>();
    let concept_scores = state
        .activation_concept_graph
        .concept_activation(&submodule_concepts)
        .await
        .unwrap_or_default();
    map_module_scores_from_concept_scores(active_module_names, &concept_scores)
}

fn map_module_scores_from_concept_scores(
    active_module_names: &[String],
    concept_scores: &HashMap<String, f64>,
) -> Vec<(String, f64)> {
    let mut scored = active_module_names
        .iter()
        .map(|name| {
            let concept_name = format!("submodule:{}", name);
            let score = concept_scores
                .get(concept_name.as_str())
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            (name.clone(), score)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored
}

fn select_modules_by_threshold(scores: &[(String, f64)], threshold: f32) -> Vec<String> {
    let threshold = (threshold as f64).clamp(0.0, 1.0);
    scores
        .iter()
        .filter(|(_, score)| *score >= threshold)
        .map(|(name, _)| name.clone())
        .collect()
}

async fn read_saturation_levels(
    state: &AppState,
    active_module_names: &[String],
) -> HashMap<String, f64> {
    let guard = state.submodule_saturation_levels.read().await;
    active_module_names
        .iter()
        .filter_map(|name| {
            guard
                .get(name)
                .copied()
                .map(|value| (name.clone(), value.clamp(0.0, SATURATION_MAX)))
        })
        .collect::<HashMap<_, _>>()
}

fn apply_saturation_penalty(
    scores: &[(String, f64)],
    saturation_levels: &HashMap<String, f64>,
) -> (Vec<(String, f64)>, BTreeMap<String, f64>, BTreeMap<String, f64>) {
    let mut inhibited = Vec::<(String, f64)>::with_capacity(scores.len());
    let mut penalties = BTreeMap::<String, f64>::new();
    let mut effective = BTreeMap::<String, f64>::new();
    for (name, score) in scores {
        let penalty = saturation_levels
            .get(name.as_str())
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, SATURATION_MAX);
        let adjusted = (score - penalty).clamp(0.0, 1.0);
        inhibited.push((name.clone(), adjusted));
        penalties.insert(name.clone(), penalty);
        effective.insert(name.clone(), adjusted);
    }
    inhibited.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    (inhibited, penalties, effective)
}

async fn update_saturation_levels(
    state: &AppState,
    active_module_names: &[String],
    hard_triggers: &[String],
) {
    let hard_set = hard_triggers.iter().cloned().collect::<HashSet<_>>();
    let mut guard = state.submodule_saturation_levels.write().await;
    for name in active_module_names {
        let current = guard
            .get(name.as_str())
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, SATURATION_MAX);
        let next = if hard_set.contains(name.as_str()) {
            (current + SATURATION_STEP).min(SATURATION_MAX)
        } else {
            (current - SATURATION_RECOVERY).max(0.0)
        };
        if next <= 0.0 {
            guard.remove(name.as_str());
        } else {
            guard.insert(name.clone(), next);
        }
    }
}

async fn dampen_hard_triggered_submodule_arousal(state: &AppState, hard_triggers: &[String]) {
    for module_name in hard_triggers {
        let concept = format!("submodule:{}", module_name);
        if let Err(err) = state
            .activation_concept_graph
            .dampen_concept_arousal(concept, POST_HARD_DAMPEN_RATIO)
            .await
        {
            emit_router_debug_error(state, "dampen_concept_arousal", &err).await;
        }
    }
}

fn collect_submodule_activation_sources(selected_seeds: &[String]) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for seed in selected_seeds {
        let trimmed = seed.trim();
        if trimmed.is_empty() || trimmed.starts_with("submodule:") {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn build_router_config(
    instructions: String,
    runtime: &ModuleRuntime,
    router_model: &str,
) -> ResponseApiConfig {
    ResponseApiConfig {
        model: router_model.to_string(),
        instructions,
        temperature: runtime.temperature,
        max_output_tokens: runtime.max_output_tokens,
        tools: Vec::new(),
        tool_handler: None,
        max_tool_rounds: 0,
    }
}

async fn emit_router_debug_raw(
    state: &AppState,
    context: &str,
    raw: &serde_json::Value,
    output_text: &str,
    tool_calls: &[crate::llm::ToolCallTrace],
) {
    let event = build_event(
        "router",
        "text",
        json!({
            "raw": raw,
            "context": context,
            "output_text": output_text,
            "tool_calls": tool_calls,
            "mode": "runtime",
        }),
        vec![
            "debug".to_string(),
            "llm.raw".to_string(),
            "mode:runtime".to_string(),
        ],
    );
    record_event(state, event).await;
}

async fn emit_router_debug_error(state: &AppState, context: &str, error: &str) {
    let event = build_event(
        "router",
        "text",
        json!({
            "mode": "runtime",
            "context": context,
            "error": error,
        }),
        vec![
            "debug".to_string(),
            "llm.error".to_string(),
            "error".to_string(),
            "mode:runtime".to_string(),
        ],
    );
    record_event(state, event).await;
}

async fn emit_concept_graph_query_event(
    state: &AppState,
    query_terms: &[String],
    limit: usize,
    candidate_concepts: &[String],
    selected_seeds: &[String],
    active_concepts_from_concept_graph: &str,
) {
    let event = build_event(
        "router",
        "state",
        json!({
            "query_terms": query_terms,
            "limit": limit,
            "result_concepts": candidate_concepts,
            "selected_seeds": selected_seeds,
            "active_concepts_from_concept_graph": active_concepts_from_concept_graph,
        }),
        vec!["debug".to_string(), "concept_graph.query".to_string()],
    );
    record_event(state, event).await;
}

async fn preprocess_router_activation(
    input_text: &str,
    concept_limit: usize,
    state: &AppState,
) -> RouterPreprocessOutput {
    let query_terms = extract_query_terms_v0(input_text, concept_limit);
    if query_terms.is_empty() {
        return RouterPreprocessOutput {
            query_terms,
            candidate_concepts: Vec::new(),
        };
    }
    let candidate_concepts = state
        .activation_concept_graph
        .concept_search(&query_terms, concept_limit)
        .await
        .unwrap_or_default();
    RouterPreprocessOutput {
        query_terms,
        candidate_concepts,
    }
}

async fn resolve_active_concepts_from_concept_graph(
    input_text: &str,
    active_module_names: &[String],
    preprocess: &RouterPreprocessOutput,
    concept_limit: usize,
    modules: &Modules,
    state: &AppState,
    overrides: &PromptOverrides,
) -> RouterConceptResolution {
    let started = Instant::now();
    let module_lines = if active_module_names.is_empty() {
        "none".to_string()
    } else {
        active_module_names
            .iter()
            .map(|name| format!("- {}", name))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let query_term_lines = render_list_for_prompt(&preprocess.query_terms);
    let candidate_lines = render_list_for_prompt(&preprocess.candidate_concepts);
    let context = render_router_context_template(
        &state.input.router_context_template,
        input_text,
        &module_lines,
        &query_term_lines,
        &candidate_lines,
        state.router.query_terms_max.max(1),
    );
    let base_instructions = overrides
        .base
        .clone()
        .unwrap_or_else(|| modules.runtime.base_instructions.clone());
    let router_instructions = overrides
        .router
        .clone()
        .unwrap_or_else(|| state.router_instructions.clone());
    let instructions = format!(
        "{}\n\n{}\n\nYou are the router preconscious module. Select recall seed concepts only.",
        base_instructions, router_instructions
    );
    let adapter = ResponseApiAdapter::new(build_router_config(
        instructions,
        &modules.runtime,
        state.router_model.as_str(),
    ));
    let llm_started = Instant::now();
    let response = match adapter
        .respond(LlmRequest {
            input: context.clone(),
        })
        .await
    {
        Ok(response) => {
            println!(
                "PERF router.llm stage=respond ms={} ok=true output_len={} tool_calls={}",
                llm_started.elapsed().as_millis(),
                response.text.len(),
                response.tool_calls.len()
            );
            response
        }
        Err(err) => {
            let detail = err.to_string();
            println!(
                "PERF router.llm stage=respond ms={} ok=false error={}",
                llm_started.elapsed().as_millis(),
                detail
            );
            emit_router_debug_error(state, &context, &detail).await;
            return RouterConceptResolution {
                selected_seeds: Vec::new(),
                active_concepts_from_concept_graph: "none".to_string(),
            };
        }
    };
    emit_router_debug_raw(
        state,
        &context,
        &response.raw,
        &response.text,
        &response.tool_calls,
    )
    .await;
    let selected_seeds = parse_recall_seeds(&response.text, concept_limit);
    let active_concepts_from_concept_graph = if selected_seeds.is_empty() {
        "none".to_string()
    } else {
        match state
            .activation_concept_graph
            .recall_query(selected_seeds.clone(), ROUTER_RECALL_MAX_HOP)
            .await
        {
            Ok(value) => render_recall_result_as_active_concepts(&value, concept_limit),
            Err(err) => {
                emit_router_debug_error(
                    state,
                    &format!("recall_query seeds={:?}", selected_seeds),
                    &err,
                )
                .await;
                "none".to_string()
            }
        }
    };
    if active_concepts_from_concept_graph == "none" {
        println!(
            "PERF router stage=resolve_concepts total_ms={} output=none_empty",
            started.elapsed().as_millis()
        );
    } else {
        println!(
            "PERF router stage=resolve_concepts total_ms={} output_len={}",
            started.elapsed().as_millis(),
            active_concepts_from_concept_graph.len()
        );
    }
    RouterConceptResolution {
        selected_seeds,
        active_concepts_from_concept_graph,
    }
}

fn render_list_for_prompt(values: &[String]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values
        .iter()
        .map(|value| format!("- {}", value))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_recall_seeds(text: &str, max: usize) -> Vec<String> {
    let mut seeds = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for raw in text.lines() {
        let mut line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("none") || line.eq_ignore_ascii_case("(empty response)") {
            continue;
        }
        line = line
            .trim_start_matches("-")
            .trim_start_matches("*")
            .trim_start_matches("•")
            .trim();
        line = line
            .strip_prefix("seed=")
            .or_else(|| line.strip_prefix("seed:"))
            .unwrap_or(line)
            .trim();
        if let Some((prefix, _)) = line.split_once("\tscore=") {
            line = prefix.trim();
        }
        line = line.trim_matches('"').trim_matches('\'').trim();
        if line.is_empty() {
            continue;
        }
        let key = line.to_lowercase();
        if seen.insert(key) {
            seeds.push(line.to_string());
        }
        if seeds.len() >= max {
            break;
        }
    }
    seeds
}

fn render_recall_result_as_active_concepts(value: &Value, max_lines: usize) -> String {
    let lines = value
        .get("propositions")
        .and_then(|items| items.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let text = item.get("text").and_then(|value| value.as_str())?.trim();
                    if text.is_empty() {
                        return None;
                    }
                    let score = item
                        .get("score")
                        .and_then(|value| value.as_f64())
                        .unwrap_or(0.0);
                    let clamped = score.clamp(0.0, 1.0);
                    Some(format!("{}\tscore={:.2}", text, clamped))
                })
                .take(max_lines.max(1))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if lines.is_empty() {
        "none".to_string()
    } else {
        lines.join("\n")
    }
}

fn extract_query_terms_v0(input_text: &str, max_terms: usize) -> Vec<String> {
    let normalized = normalize_input_text_v0(input_text);
    if normalized.is_empty() {
        return Vec::new();
    }
    let segments = segment_by_char_class(&normalized);
    let mut terms = Vec::<String>::new();
    for segment in segments {
        append_segment_variants(&mut terms, &segment);
    }
    dedupe_and_filter_terms(terms, max_terms.max(1))
}

fn normalize_input_text_v0(input_text: &str) -> String {
    let mut chars = String::with_capacity(input_text.len());
    for ch in input_text.chars() {
        let normalized = normalize_full_width_char(ch);
        if normalized.is_whitespace() || is_punctuation_like(normalized) {
            chars.push(' ');
        } else {
            chars.push(normalized);
        }
    }
    chars.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_full_width_char(ch: char) -> char {
    match ch {
        '\u{3000}' => ' ',
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        _ => ch,
    }
}

fn is_punctuation_like(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '。' | '、'
                | '・'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '【'
                | '】'
                | '［'
                | '］'
                | '｛'
                | '｝'
                | '〈'
                | '〉'
                | '《'
                | '》'
                | '！'
                | '？'
                | '：'
                | '；'
                | '，'
                | '．'
                | '〜'
                | '～'
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Kanji,
    Hiragana,
    Katakana,
    Alnum,
    Other,
}

fn classify_char(ch: char) -> CharClass {
    if ch.is_ascii_alphanumeric() || ('０'..='９').contains(&ch) || ('ａ'..='ｚ').contains(&ch)
    {
        return CharClass::Alnum;
    }
    if ('\u{4E00}'..='\u{9FFF}').contains(&ch) || ('\u{3400}'..='\u{4DBF}').contains(&ch) {
        return CharClass::Kanji;
    }
    if ('\u{3040}'..='\u{309F}').contains(&ch) {
        return CharClass::Hiragana;
    }
    if ('\u{30A0}'..='\u{30FF}').contains(&ch) || ('\u{31F0}'..='\u{31FF}').contains(&ch) {
        return CharClass::Katakana;
    }
    CharClass::Other
}

fn segment_by_char_class(input: &str) -> Vec<String> {
    let mut segments = Vec::<String>::new();
    let mut current = String::new();
    let mut current_class = CharClass::Other;
    for ch in input.chars() {
        let class = classify_char(ch);
        if class == CharClass::Other {
            if !current.is_empty() {
                segments.push(current.clone());
                current.clear();
            }
            current_class = CharClass::Other;
            continue;
        }
        if current.is_empty() || class == current_class {
            current.push(ch);
            current_class = class;
            continue;
        }
        segments.push(current.clone());
        current.clear();
        current.push(ch);
        current_class = class;
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn append_segment_variants(out: &mut Vec<String>, segment: &str) {
    let chars = segment.chars().collect::<Vec<_>>();
    let len = chars.len();
    if len == 0 {
        return;
    }
    out.push(segment.to_string());
    if len >= 3 {
        out.push(chars[..(len - 1)].iter().collect::<String>());
        out.push(chars[1..].iter().collect::<String>());
    }
    if len >= 4 {
        out.push(chars[1..(len - 1)].iter().collect::<String>());
    }
}

fn dedupe_and_filter_terms(terms: Vec<String>, max_terms: usize) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for term in terms {
        let cleaned = term.trim();
        if !is_valid_query_term(cleaned) {
            continue;
        }
        let key = cleaned.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(cleaned.to_string());
        if out.len() >= max_terms {
            break;
        }
    }
    out
}

fn is_valid_query_term(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if value.chars().count() == 1 {
        return value
            .chars()
            .next()
            .map(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
            .unwrap_or(false);
    }
    value
        .chars()
        .all(|ch| classify_char(ch) != CharClass::Other && !ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_modules_by_threshold_filters_by_score() {
        let scores = vec![
            ("curiosity".to_string(), 0.92),
            ("self_preservation".to_string(), 0.72),
            ("social_approval".to_string(), 0.58),
        ];
        let hard = select_modules_by_threshold(&scores, 0.85);
        let soft = select_modules_by_threshold(&scores, 0.60);
        assert_eq!(hard, vec!["curiosity".to_string()]);
        assert_eq!(
            soft,
            vec!["curiosity".to_string(), "self_preservation".to_string()]
        );
    }

    #[test]
    fn extract_query_terms_v0_splits_and_dedupes() {
        let terms = extract_query_terms_v0("京都旅行プラン2026!!", 8);
        assert!(!terms.is_empty());
        assert!(terms.iter().any(|term| term == "京都旅行"));
        assert!(terms.iter().any(|term| term == "プラン"));
        assert!(terms.iter().any(|term| term == "2026"));
    }

    #[test]
    fn parse_recall_seeds_ignores_noise() {
        let text = "- seed=京都\n* 大阪\tscore=0.42\nnone\n";
        let seeds = parse_recall_seeds(text, 4);
        assert_eq!(seeds, vec!["京都".to_string(), "大阪".to_string()]);
    }

    #[test]
    fn render_recall_result_as_active_concepts_formats_lines() {
        let value = json!({
            "propositions": [
                { "text": "京都 evokes 寺", "score": 0.8231 }
            ]
        });
        let text = render_recall_result_as_active_concepts(&value, 8);
        assert_eq!(text, "京都 evokes 寺\tscore=0.82");
    }

    #[test]
    fn map_module_scores_reads_submodule_concept_activation_only() {
        let modules = vec![
            "curiosity".to_string(),
            "self_preservation".to_string(),
            "social_approval".to_string(),
        ];
        let concept_scores = HashMap::from([
            ("submodule:curiosity".to_string(), 0.91),
            ("submodule:self_preservation".to_string(), 0.41),
            ("関係ない概念".to_string(), 1.0),
        ]);
        let scores = map_module_scores_from_concept_scores(&modules, &concept_scores);
        assert_eq!(
            scores,
            vec![
                ("curiosity".to_string(), 0.91),
                ("self_preservation".to_string(), 0.41),
                ("social_approval".to_string(), 0.0),
            ]
        );
    }
}
