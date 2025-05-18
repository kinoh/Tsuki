use crate::adapter::dify::CodeExecutor;
use crate::adapter::openai::Function;
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ExecuteCodeFunctionArguments {
    pub code: String,
}

pub struct ExecuteCodeFunction {
    pub client: CodeExecutor,
}

#[async_trait]
impl Function for ExecuteCodeFunction {
    fn name(&self) -> &'static str {
        "execute_code"
    }

    fn description(&self) -> &'static str {
        "Execute Python code. you must print() output; only stdout is returned. available packages: requests certifi beautifulsoup4 numpy scipy pandas scikit-learn matplotlib lxml pypdf"
    }

    fn args_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "code to execute"
                }
            },
            "required": ["code"],
            "additionalProperties": false
        })
    }

    async fn call(&self, args_json: &str) -> Result<String, String> {
        let args: ExecuteCodeFunctionArguments =
            serde_json::from_str(&args_json).map_err(|_| "invalid arguments".to_string())?;
        self.client
            .execute(&args.code)
            .await
            .map_err(|e| e.to_string())
    }
}
