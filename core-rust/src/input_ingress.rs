use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MediaAttachment {
    pub(crate) data: String,
    #[serde(rename = "mimeType")]
    pub(crate) mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RouterInput {
    pub(crate) kind: String,
    pub(crate) text: String,
    pub(crate) images: Vec<MediaAttachment>,
    pub(crate) audio: Vec<MediaAttachment>,
}

impl RouterInput {
    pub(crate) fn from_text(kind: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            text: text.into(),
            images: Vec::new(),
            audio: Vec::new(),
        }
    }

    pub(crate) fn new(
        kind: impl Into<String>,
        text: impl Into<String>,
        images: Vec<MediaAttachment>,
        audio: Vec<MediaAttachment>,
    ) -> Self {
        Self {
            kind: kind.into(),
            text: text.into(),
            images: normalize_media(images),
            audio: normalize_media(audio),
        }
    }

    pub(crate) fn trimmed_text(&self) -> &str {
        self.text.trim()
    }

    pub(crate) fn has_media(&self) -> bool {
        !self.images.is_empty() || !self.audio.is_empty()
    }

    pub(crate) fn display_text(&self) -> String {
        let trimmed = self.trimmed_text();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }

        let mut parts = Vec::<String>::new();
        if !self.images.is_empty() {
            let suffix = if self.images.len() == 1 { "" } else { "s" };
            parts.push(format!("{} image{}", self.images.len(), suffix));
        }
        if !self.audio.is_empty() {
            let suffix = if self.audio.len() == 1 { "" } else { "s" };
            parts.push(format!("{} audio clip{}", self.audio.len(), suffix));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("[sensory input: {}]", parts.join(", "))
        }
    }

    pub(crate) fn event_payload(&self) -> Value {
        let mut payload = Map::<String, Value>::new();
        payload.insert("text".to_string(), Value::String(self.display_text()));
        if !self.trimmed_text().is_empty() {
            payload.insert(
                "user_text".to_string(),
                Value::String(self.trimmed_text().to_string()),
            );
        }
        if !self.images.is_empty() {
            payload.insert("images".to_string(), json!(self.images));
        }
        if !self.audio.is_empty() {
            payload.insert("audio".to_string(), json!(self.audio));
        }
        Value::Object(payload)
    }
}

fn normalize_media(items: Vec<MediaAttachment>) -> Vec<MediaAttachment> {
    items
        .into_iter()
        .filter_map(|item| {
            let data = item.data.trim().to_string();
            let mime_type = item.mime_type.trim().to_string();
            if data.is_empty() || mime_type.is_empty() {
                None
            } else {
                Some(MediaAttachment { data, mime_type })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{MediaAttachment, RouterInput};

    #[test]
    fn display_text_prefers_user_text() {
        let input = RouterInput::new("sensory", "  look at this  ", Vec::new(), Vec::new());
        assert_eq!(input.display_text(), "look at this");
    }

    #[test]
    fn display_text_summarizes_media_only_input() {
        let input = RouterInput::new(
            "sensory",
            "",
            vec![MediaAttachment {
                data: "abc".to_string(),
                mime_type: "image/png".to_string(),
            }],
            vec![MediaAttachment {
                data: "def".to_string(),
                mime_type: "audio/wav".to_string(),
            }],
        );
        assert_eq!(
            input.display_text(),
            "[sensory input: 1 image, 1 audio clip]"
        );
    }
}
