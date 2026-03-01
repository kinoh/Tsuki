use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

use crate::application::execution_service::{
    current_prompt_overrides, load_active_module_instructions, run_all_submodules_debug,
    run_decision_debug, run_submodule_debug, run_submodule_tool,
};
use crate::application::history_service::{is_decision_event, is_user_input_event, latest_events};
use crate::application::router_service::run_router;
use crate::event::build_event;
use crate::{record_event, AppState, DebugRunRequest, DebugRunResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppendInputMode {
    AlwaysNew,
    ReuseOpen,
}

impl AppendInputMode {
    fn from_request(value: Option<&str>) -> Self {
        match value {
            Some(raw) if raw.eq_ignore_ascii_case("reuse_open") => Self::ReuseOpen,
            _ => Self::AlwaysNew,
        }
    }
}

#[derive(Debug, Deserialize)]
struct InputMessage {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
enum ParsedIngress {
    Trigger { event: String, payload: Value },
    Input { kind: String, text: String },
}

pub(crate) async fn run_debug_module(
    state: &AppState,
    name: String,
    payload: DebugRunRequest,
) -> Result<DebugRunResponse, (StatusCode, String)> {
    let context_override = payload
        .context_override
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if payload.input.trim().is_empty() && context_override.is_none() {
        return Err((StatusCode::BAD_REQUEST, "input is required".to_string()));
    }
    if context_override.is_some() && name == "submodules" {
        return Err((
            StatusCode::BAD_REQUEST,
            "context_override is not supported for submodules".to_string(),
        ));
    }
    let include_history = payload.include_history.unwrap_or(true);
    let history_cutoff_ts = payload.history_cutoff_ts.as_deref();
    let excluded_event_ids = payload
        .exclude_event_ids
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    let append_mode = AppendInputMode::from_request(payload.append_input_mode.as_deref());
    if context_override.is_none() {
        maybe_append_debug_input_event(
            state,
            payload.input.trim(),
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            append_mode,
        )
        .await;
    }

    let overrides = current_prompt_overrides(state).await;
    let module_instructions = load_active_module_instructions(state, &overrides).await;
    let input_text = payload.input.clone();
    let router_output = run_router(
        &input_text,
        &module_instructions,
        &state.modules,
        state,
        &overrides,
        |module_name, activation_snapshot, instructions, focus| {
            let module_name = module_name.to_string();
            let activation_snapshot = activation_snapshot.clone();
            let instructions = instructions.to_string();
            let focus = focus.map(str::to_string);
            let input_text = input_text.clone();
            async move {
                run_submodule_tool(
                    state,
                    &input_text,
                    &activation_snapshot,
                    &module_name,
                    &instructions,
                    focus.as_deref(),
                )
                .await
            }
        },
    )
    .await;

    let output = if name == "decision" {
        run_decision_debug(
            &payload.input,
            context_override,
            payload.submodule_outputs.as_deref(),
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            state,
            &router_output,
            &module_instructions,
            &overrides,
        )
        .await?
    } else if name == "submodules" {
        run_all_submodules_debug(
            &payload.input,
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            state,
        )
        .await?
    } else {
        run_submodule_debug(
            &name,
            &payload.input,
            context_override,
            include_history,
            history_cutoff_ts,
            &excluded_event_ids,
            state,
        )
        .await?
    };
    Ok(DebugRunResponse { output })
}

pub(crate) async fn parse_and_append_input(raw: &str, state: &AppState) -> Result<String, ()> {
    let ingress = match parse_input_message(raw) {
        Ok(value) => value,
        Err(message) => {
            let event = build_event(
                "system",
                "text",
                json!({ "text": message }),
                vec!["error".to_string()],
            );
            record_event(state, event).await;
            return Err(());
        }
    };

    match ingress {
        ParsedIngress::Trigger { event, payload } => {
            let trigger_event = build_event("system", "text", payload, vec![event]);
            record_event(state, trigger_event).await;
            return Err(());
        }
        ParsedIngress::Input { kind, text } => {
            let input_event = build_event(
                "user",
                "text",
                json!({ "text": text }),
                vec!["input".to_string(), format!("type:{}", kind)],
            );
            record_event(state, input_event.clone()).await;

            Ok(input_event
                .payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string())
        }
    }
}

fn parse_input_message(raw: &str) -> Result<ParsedIngress, &'static str> {
    let input: InputMessage = serde_json::from_str(raw).map_err(|_| "invalid input payload")?;

    let kind = if input.kind.trim().is_empty() {
        "message".to_string()
    } else {
        input.kind.trim().to_string()
    };

    if kind == "trigger" {
        let event = input
            .event
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_default();
        if event.is_empty() {
            return Err("trigger event is required");
        }
        return Ok(ParsedIngress::Trigger {
            event,
            payload: input
                .payload
                .unwrap_or_else(|| Value::Object(Default::default())),
        });
    }

    if kind != "message" && kind != "sensory" {
        return Err("invalid input type");
    }

    Ok(ParsedIngress::Input {
        kind,
        text: input.text,
    })
}

async fn maybe_append_debug_input_event(
    state: &AppState,
    input_text: &str,
    include_history: bool,
    cutoff_ts: Option<&str>,
    excluded_event_ids: &HashSet<String>,
    append_mode: AppendInputMode,
) {
    let normalized_input = input_text.trim();
    if normalized_input.is_empty() {
        return;
    }
    let should_append = match append_mode {
        AppendInputMode::AlwaysNew => true,
        AppendInputMode::ReuseOpen => {
            if !include_history {
                true
            } else {
                let events = latest_events(state, 1000, cutoff_ts, Some(excluded_event_ids)).await;
                should_append_debug_input_for_reuse_open(normalized_input, &events)
            }
        }
    };
    if !should_append {
        return;
    }
    let event = build_event(
        "user",
        "text",
        json!({ "text": normalized_input }),
        vec!["input".to_string(), "type:message".to_string()],
    );
    record_event(state, event).await;
}

fn should_append_debug_input_for_reuse_open(
    input_text: &str,
    events: &[crate::event::Event],
) -> bool {
    let mut saw_decision_after_input = false;
    for event in events {
        if is_decision_event(event) {
            saw_decision_after_input = true;
            continue;
        }
        if !is_user_input_event(event) {
            continue;
        }
        if saw_decision_after_input {
            return true;
        }
        let previous_input = event
            .payload
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim();
        return previous_input != input_text;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{parse_input_message, ParsedIngress};
    use serde_json::json;

    #[test]
    fn parse_input_accepts_default_message_kind() {
        let parsed = parse_input_message(r#"{"text":"hello"}"#).expect("must parse");
        assert_eq!(
            parsed,
            ParsedIngress::Input {
                kind: "message".to_string(),
                text: "hello".to_string(),
            }
        );
    }

    #[test]
    fn parse_input_accepts_sensory_kind() {
        let parsed =
            parse_input_message(r#"{"type":"sensory","text":"rain"}"#).expect("must parse");
        assert_eq!(
            parsed,
            ParsedIngress::Input {
                kind: "sensory".to_string(),
                text: "rain".to_string(),
            }
        );
    }

    #[test]
    fn parse_input_rejects_unknown_kind() {
        let err = parse_input_message(r#"{"type":"unknown","text":"x"}"#).expect_err("must fail");
        assert_eq!(err, "invalid input type");
    }

    #[test]
    fn parse_input_accepts_trigger_event() {
        let parsed = parse_input_message(
            r#"{"type":"trigger","event":"self_improvement.run","payload":{"target":"router"}}"#,
        )
        .expect("must parse");
        assert_eq!(
            parsed,
            ParsedIngress::Trigger {
                event: "self_improvement.run".to_string(),
                payload: json!({"target":"router"}),
            }
        );
    }

    #[test]
    fn parse_input_rejects_trigger_without_event() {
        let err = parse_input_message(r#"{"type":"trigger","payload":{"target":"router"}}"#)
            .expect_err("must fail");
        assert_eq!(err, "trigger event is required");
    }

    #[test]
    fn parse_input_rejects_invalid_json() {
        let err = parse_input_message("not-json").expect_err("must fail");
        assert_eq!(err, "invalid input payload");
    }
}
