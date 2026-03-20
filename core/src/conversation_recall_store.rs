use async_trait::async_trait;

use crate::event::Event;

#[derive(Debug, Clone)]
pub(crate) struct ConversationRecallCandidate {
    pub(crate) event_id: String,
    pub(crate) semantic_similarity: f64,
}

#[async_trait]
pub(crate) trait ConversationRecallStore: Send + Sync {
    async fn upsert_event_projection(&self, event: &Event) -> Result<(), String>;
    async fn search_event_projections(
        &self,
        input_text: &str,
        limit: usize,
    ) -> Result<Vec<ConversationRecallCandidate>, String>;
}

pub(crate) fn conversation_recall_text(event: &Event) -> Option<String> {
    if event.modality != "text" {
        return None;
    }
    let text = event
        .payload
        .get("text")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let is_user_input = event.source == "user"
        && event
            .meta
            .tags
            .iter()
            .any(|tag| tag == "input" || tag == "user_input");
    let is_assistant_response =
        event.source == "assistant" && event.meta.tags.iter().any(|tag| tag == "response");
    if is_user_input || is_assistant_response {
        Some(text.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::conversation_recall_text;
    use crate::event::contracts::{input_text, response_text};
    use crate::event::rehydrate_event;
    use serde_json::json;

    #[test]
    fn accepts_user_input_and_response() {
        let input = input_text("user", "message", "hello");
        let response = response_text("hi".to_string());
        assert_eq!(conversation_recall_text(&input).as_deref(), Some("hello"));
        assert_eq!(conversation_recall_text(&response).as_deref(), Some("hi"));
    }

    #[test]
    fn accepts_imported_legacy_user_rows() {
        let event = rehydrate_event(
            "event-1".to_string(),
            "2026-03-09T00:00:00Z".to_string(),
            "user".to_string(),
            "text".to_string(),
            json!({ "text": "legacy hello" }),
            vec!["imported_legacy".to_string(), "user_input".to_string()],
        );
        assert_eq!(
            conversation_recall_text(&event).as_deref(),
            Some("legacy hello")
        );
    }

    #[test]
    fn rejects_internal_events() {
        let event = rehydrate_event(
            "event-2".to_string(),
            "2026-03-09T00:00:00Z".to_string(),
            "decision".to_string(),
            "text".to_string(),
            json!({ "text": "decision=respond" }),
            vec!["decision".to_string()],
        );
        assert!(conversation_recall_text(&event).is_none());
    }
}
