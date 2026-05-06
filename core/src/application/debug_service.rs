use serde::Deserialize;
use serde_json::Value;

use crate::app_state::AppState;
use crate::application::event_service::record_event;
use crate::event::contracts::{
    input_sensory as emit_input_sensory, input_text as emit_input_text, named_trigger, parse_error,
};
use crate::input_ingress::{MediaAttachment, RouterInput};

#[derive(Debug, Deserialize)]
struct InputMessage {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    images: Vec<MediaAttachment>,
    #[serde(default)]
    audio: Vec<MediaAttachment>,
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
enum ParsedIngress {
    Trigger { event: String, payload: Value },
    Input { input: RouterInput },
}

pub(crate) async fn parse_and_append_input(raw: &str, state: &AppState) -> Result<RouterInput, ()> {
    let ingress = match parse_input_message(raw) {
        Ok(value) => value,
        Err(message) => {
            let event = parse_error(message);
            record_event(state, event).await;
            return Err(());
        }
    };

    match ingress {
        ParsedIngress::Trigger { event, payload } => {
            let trigger_event = named_trigger("system", &event, payload);
            record_event(state, trigger_event).await;
            return Err(());
        }
        ParsedIngress::Input { input } => {
            let source = if input.kind == "scheduler_notice" {
                "system"
            } else {
                "user"
            };
            let display_text = input.display_text();
            let input_event = if input.has_media() || input.kind == "sensory" {
                emit_input_sensory(source, input.kind.as_str(), input.event_payload())
            } else {
                emit_input_text(source, input.kind.as_str(), display_text.as_str())
            };
            record_event(state, input_event.clone()).await;

            Ok(input)
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

    let router_input = RouterInput::new(kind, input.text, input.images, input.audio);
    if router_input.display_text().is_empty() {
        return Err("input text or sensory media is required");
    }
    Ok(ParsedIngress::Input {
        input: router_input,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_input_message, ParsedIngress};
    use crate::input_ingress::{MediaAttachment, RouterInput};
    use serde_json::json;

    #[test]
    fn parse_input_accepts_default_message_kind() {
        let parsed = parse_input_message(r#"{"text":"hello"}"#).expect("must parse");
        assert_eq!(
            parsed,
            ParsedIngress::Input {
                input: RouterInput::from_text("message", "hello"),
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
                input: RouterInput::from_text("sensory", "rain"),
            }
        );
    }

    #[test]
    fn parse_input_accepts_sensory_media_without_text() {
        let parsed = parse_input_message(
            r#"{"type":"sensory","images":[{"data":"abc","mimeType":"image/png"}]}"#,
        )
        .expect("must parse");
        assert_eq!(
            parsed,
            ParsedIngress::Input {
                input: RouterInput::new(
                    "sensory",
                    "",
                    vec![MediaAttachment {
                        data: "abc".to_string(),
                        mime_type: "image/png".to_string(),
                    }],
                    Vec::new(),
                ),
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
