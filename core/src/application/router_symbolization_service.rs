use crate::input_ingress::RouterInput;
use crate::router_symbolizer::RouterSymbolizer;

pub(crate) struct SymbolizationResult {
    pub(crate) text: String,
    pub(crate) error: Option<String>,
}

pub(crate) async fn symbolize(
    input: &RouterInput,
    symbolizer: &dyn RouterSymbolizer,
) -> SymbolizationResult {
    match symbolizer.symbolize(input).await {
        Ok(text) => SymbolizationResult { text, error: None },
        Err(err) => SymbolizationResult {
            text: input.display_text(),
            error: Some(err),
        },
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;

    struct MockSymbolizer {
        result: Result<String, String>,
    }

    #[async_trait]
    impl RouterSymbolizer for MockSymbolizer {
        async fn symbolize(&self, _input: &RouterInput) -> Result<String, String> {
            self.result.clone()
        }
    }

    #[tokio::test]
    async fn returns_symbolizer_output_on_success() {
        let symbolizer = MockSymbolizer {
            result: Ok("静かな夜の公園、遠くで虫の声".to_string()),
        };
        let input = RouterInput::from_text("user", "これ見て");
        let result = symbolize(&input, &symbolizer).await;
        assert_eq!(result.text, "静かな夜の公園、遠くで虫の声");
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn falls_back_to_display_text_on_error() {
        let symbolizer = MockSymbolizer {
            result: Err("backend unavailable".to_string()),
        };
        let input = RouterInput::from_text("user", "こんにちは");
        let result = symbolize(&input, &symbolizer).await;
        assert_eq!(result.text, "こんにちは");
        assert_eq!(result.error.as_deref(), Some("backend unavailable"));
    }
}
