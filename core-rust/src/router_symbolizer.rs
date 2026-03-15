use async_openai::{
    config::OpenAIConfig,
    types::responses::{
        CreateResponseArgs, EasyInputContent, EasyInputMessage, ImageDetail, InputContent,
        InputImageContent, InputParam, InputTextContent, MessageType, OutputItem,
        OutputMessageContent, Role,
    },
    Client,
};
use async_trait::async_trait;

use crate::input_ingress::{MediaAttachment, RouterInput};

const SYMBOLIZER_INSTRUCTIONS: &str = "\
You are a router symbolizer. Describe the provided input literally and concisely in plain text. \
Include what you observe factually and the sensory impression it conveys. \
Output only the description, no commentary.";

/// Production backend that calls the OpenAI Responses API with vision support.
pub(crate) struct ResponseApiSymbolizerBackend {
    client: Client<OpenAIConfig>,
    model: String,
}

impl ResponseApiSymbolizerBackend {
    pub(crate) fn new(model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            model: model.into(),
        }
    }
}

#[async_trait]
impl SymbolizerBackend for ResponseApiSymbolizerBackend {
    async fn describe(&self, input: &RouterInput) -> Result<String, String> {
        let mut content = Vec::<InputContent>::new();

        if !input.trimmed_text().is_empty() {
            content.push(InputContent::InputText(InputTextContent {
                text: input.trimmed_text().to_string(),
            }));
        }

        for image in &input.images {
            content.push(InputContent::InputImage(image_content(image)));
        }

        // Audio attachments: pass as text note since the Responses API does not expose an
        // InputAudio content part. A future iteration can use InputFile when supported.
        if !input.audio.is_empty() {
            let note = format!("[{} audio clip(s) provided]", input.audio.len());
            content.push(InputContent::InputText(InputTextContent {
                text: note,
            }));
        }

        let message = EasyInputMessage {
            r#type: MessageType::Message,
            role: Role::User,
            content: EasyInputContent::ContentList(content),
        };
        let input_param = InputParam::Items(vec![message.into()]);

        let built = CreateResponseArgs::default()
            .model(self.model.as_str())
            .instructions(SYMBOLIZER_INSTRUCTIONS)
            .input(input_param)
            .build()
            .map_err(|err| format!("symbolizer request build failed: {}", err))?;

        let response = self
            .client
            .responses()
            .create(built)
            .await
            .map_err(|err| format!("symbolizer api call failed: {}", err))?;

        let text = response
            .output
            .iter()
            .find_map(|item| {
                if let OutputItem::Message(msg) = item {
                    msg.content.iter().find_map(|c| {
                        if let OutputMessageContent::OutputText(t) = c {
                            Some(t.text.clone())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            })
            .unwrap_or_default();

        Ok(text)
    }
}

fn image_content(attachment: &MediaAttachment) -> InputImageContent {
    let url = format!(
        "data:{};base64,{}",
        attachment.mime_type, attachment.data
    );
    InputImageContent {
        detail: ImageDetail::Auto,
        file_id: None,
        image_url: Some(url),
    }
}

pub(crate) fn build_response_api_symbolizer(
    model: impl Into<String>,
) -> OpenAIRouterSymbolizer<ResponseApiSymbolizerBackend> {
    OpenAIRouterSymbolizer::new(ResponseApiSymbolizerBackend::new(model))
}

/// Converts a RouterInput into a literal text description for embedding and decision context.
/// Text-only inputs are returned as-is. Image and audio inputs are described via an LLM backend.
#[async_trait]
pub(crate) trait RouterSymbolizer: Send + Sync {
    async fn symbolize(&self, input: &RouterInput) -> Result<String, String>;
}

/// Backend responsible for producing a literal description from multimodal content.
#[async_trait]
pub(crate) trait SymbolizerBackend: Send + Sync {
    async fn describe(&self, input: &RouterInput) -> Result<String, String>;
}

pub(crate) struct OpenAIRouterSymbolizer<B: SymbolizerBackend> {
    backend: B,
}

impl<B: SymbolizerBackend> OpenAIRouterSymbolizer<B> {
    pub(crate) fn new(backend: B) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl<B: SymbolizerBackend> RouterSymbolizer for OpenAIRouterSymbolizer<B> {
    async fn symbolize(&self, input: &RouterInput) -> Result<String, String> {
        if !input.has_media() {
            return Ok(input.trimmed_text().to_string());
        }
        self.backend.describe(input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input_ingress::MediaAttachment;

    struct MockBackend {
        result: Result<String, String>,
        called: std::sync::Mutex<bool>,
    }

    impl MockBackend {
        fn new(result: Result<String, String>) -> Self {
            Self {
                result,
                called: std::sync::Mutex::new(false),
            }
        }

        fn was_called(&self) -> bool {
            *self.called.lock().unwrap()
        }
    }

    #[async_trait]
    impl SymbolizerBackend for MockBackend {
        async fn describe(&self, _input: &RouterInput) -> Result<String, String> {
            *self.called.lock().unwrap() = true;
            self.result.clone()
        }
    }

    fn image_input(text: &str) -> RouterInput {
        RouterInput::new(
            "sensory",
            text,
            vec![MediaAttachment {
                data: "base64data".to_string(),
                mime_type: "image/png".to_string(),
            }],
            Vec::new(),
        )
    }

    #[tokio::test]
    async fn text_only_returns_text_without_calling_backend() {
        let backend = MockBackend::new(Ok("should not appear".to_string()));
        let symbolizer = OpenAIRouterSymbolizer::new(backend);
        let input = RouterInput::from_text("user", "こんにちは");
        let result = symbolizer.symbolize(&input).await.unwrap();
        assert_eq!(result, "こんにちは");
        assert!(!symbolizer.backend.was_called());
    }

    #[tokio::test]
    async fn image_input_calls_backend_and_returns_description() {
        let backend = MockBackend::new(Ok("夕暮れの海岸、橙色の光".to_string()));
        let symbolizer = OpenAIRouterSymbolizer::new(backend);
        let result = symbolizer.symbolize(&image_input("")).await.unwrap();
        assert_eq!(result, "夕暮れの海岸、橙色の光");
        assert!(symbolizer.backend.was_called());
    }

    #[tokio::test]
    async fn image_input_with_text_calls_backend() {
        let backend = MockBackend::new(Ok("海の写真、波が穏やか".to_string()));
        let symbolizer = OpenAIRouterSymbolizer::new(backend);
        let result = symbolizer.symbolize(&image_input("これ見て")).await.unwrap();
        assert_eq!(result, "海の写真、波が穏やか");
        assert!(symbolizer.backend.was_called());
    }

    #[tokio::test]
    async fn backend_error_is_propagated() {
        let backend = MockBackend::new(Err("api error".to_string()));
        let symbolizer = OpenAIRouterSymbolizer::new(backend);
        let result = symbolizer.symbolize(&image_input("")).await;
        assert_eq!(result, Err("api error".to_string()));
    }
}
