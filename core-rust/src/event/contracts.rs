use serde_json::{json, Value};

use super::{build_event, Event};

fn emit(source: &str, modality: &str, payload: Value, tags: Vec<String>) -> Event {
    build_event(source, modality, payload, tags)
}

pub(crate) fn parse_error(text: &str) -> Event {
    emit(
        "system",
        "text",
        json!({ "text": text }),
        vec!["error".to_string()],
    )
}

pub(crate) fn named_trigger(source: &str, event_tag: &str, payload: Value) -> Event {
    emit(source, "text", payload, vec![event_tag.trim().to_string()])
}

pub(crate) fn input_text(source: &str, kind: &str, text: &str) -> Event {
    emit(
        source,
        "text",
        json!({ "text": text }),
        vec!["input".to_string(), format!("type:{}", kind)],
    )
}

pub(crate) fn input_sensory(source: &str, kind: &str, payload: Value) -> Event {
    emit(
        source,
        "sensory",
        payload,
        vec!["input".to_string(), format!("type:{}", kind)],
    )
}

pub(crate) fn response_text(text: String) -> Event {
    emit(
        "assistant",
        "text",
        json!({ "text": text }),
        vec!["response".to_string()],
    )
}

pub(crate) fn decision_text(text: String, is_error: bool) -> Event {
    let mut tags = vec!["decision".to_string()];
    if is_error {
        tags.push("error".to_string());
    }
    emit("decision", "text", json!({ "text": text }), tags)
}

pub(crate) fn role_text_output(
    source: &str,
    role_tag: &str,
    text: String,
    is_error: bool,
) -> Event {
    let mut tags = vec![role_tag.to_string()];
    if is_error {
        tags.push("error".to_string());
    }
    emit(source, "text", json!({ "text": text }), tags)
}

pub(crate) fn router_state(payload: Value) -> Event {
    emit("router", "state", payload, vec!["router".to_string()])
}

pub(crate) fn concept_graph_query(payload: Value) -> Event {
    emit(
        "router",
        "state",
        payload,
        vec!["debug".to_string(), "concept_graph.query".to_string()],
    )
}

pub(crate) fn llm_raw(source: &str, payload: Value, extra_tags: Vec<String>) -> Event {
    let mut tags = vec!["debug".to_string(), "llm.raw".to_string()];
    tags.extend(extra_tags);
    emit(source, "text", payload, tags)
}

pub(crate) fn llm_error(source: &str, payload: Value, extra_tags: Vec<String>) -> Event {
    let mut tags = vec![
        "debug".to_string(),
        "llm.error".to_string(),
        "error".to_string(),
    ];
    tags.extend(extra_tags);
    emit(source, "text", payload, tags)
}

pub(crate) fn self_improvement_module_processed(payload: Value) -> Event {
    emit(
        "self_improvement",
        "text",
        payload,
        vec!["self_improvement.module_processed".to_string()],
    )
}

pub(crate) fn self_improvement_trigger_processed(payload: Value) -> Event {
    emit(
        "self_improvement",
        "text",
        payload,
        vec![
            "self_improvement.trigger_processed".to_string(),
            "debug".to_string(),
        ],
    )
}

pub(crate) fn self_improvement_proposed(payload: Value) -> Event {
    emit(
        "system",
        "text",
        payload,
        vec!["self_improvement.proposed".to_string()],
    )
}

pub(crate) fn self_improvement_reviewed(payload: Value) -> Event {
    emit(
        "system",
        "text",
        payload,
        vec!["self_improvement.reviewed".to_string()],
    )
}

pub(crate) fn self_improvement_applied(payload: Value) -> Event {
    emit(
        "system",
        "text",
        payload,
        vec!["self_improvement.applied".to_string()],
    )
}

pub(crate) fn scheduler_notice(payload: Value) -> Event {
    emit(
        "scheduler",
        "text",
        payload,
        vec!["scheduler.notice".to_string()],
    )
}

pub(crate) fn scheduler_fired(payload: Value, related_tag: &str) -> Event {
    emit(
        "scheduler",
        "text",
        payload,
        vec!["scheduler.fired".to_string(), related_tag.to_string()],
    )
}

pub(crate) fn tool_observation(
    tool_name: &str,
    parsed_arguments: Value,
    outcome: &str,
    output: Option<&str>,
    error: Option<String>,
    elapsed_ms: u128,
) -> Event {
    emit(
        "tooling",
        "state",
        json!({
            "tool_name": tool_name,
            "arguments": parsed_arguments,
            "outcome": outcome,
            "output": output,
            "error": error,
            "elapsed_ms": elapsed_ms,
        }),
        vec![
            "observe".to_string(),
            "tool".to_string(),
            format!("tool:{}", tool_name),
            format!("outcome:{}", outcome),
        ],
    )
}
