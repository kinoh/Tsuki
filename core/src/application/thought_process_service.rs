use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::runtime::Handle;

use crate::{
    app_state::AppState,
    application::{
        event_service::record_event, history_service::format_event_lines,
        module_bootstrap::ModuleRuntime, usage_service::DbLlmUsageRecorder,
    },
    event::{contracts::response_text, Event},
    llm::{
        build_response_api_llm, LlmAdapter, LlmRequest, LlmUsageContext, LlmUsageRecorder,
        ResponseApiConfig,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct ThoughtProcessInput {
    pub(crate) events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DecisionContext {
    pub(crate) context: String,
    pub(crate) available_actions: Vec<AvailableAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AvailableAction {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DecisionOutput {
    #[serde(default)]
    pub(crate) actions: Vec<Action>,
    #[serde(default)]
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Action {
    pub(crate) name: String,
    pub(crate) input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionResult {
    pub(crate) name: String,
    pub(crate) ok: bool,
    pub(crate) output: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThoughtProcessResult {
    pub(crate) decision_context: DecisionContext,
    pub(crate) decision_output: DecisionOutput,
    pub(crate) action_results: Vec<ActionResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ThoughtProcessError {
    Cognition(String),
    Decision(DecisionError),
}

impl std::fmt::Display for ThoughtProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cognition(err) => write!(f, "cognition failed: {}", err),
            Self::Decision(err) => write!(f, "{}", err),
        }
    }
}

impl std::error::Error for ThoughtProcessError {}

#[async_trait]
pub(crate) trait CognitionComponent: Send + Sync {
    async fn build_decision_context(
        &self,
        input: &ThoughtProcessInput,
    ) -> Result<DecisionContext, String>;
}

pub(crate) struct ThoughtProcessService {
    cognition: Arc<dyn CognitionComponent>,
    decision: DecisionService,
    action_execution: ActionExecutionService,
}

impl ThoughtProcessService {
    pub(crate) fn new(
        cognition: Arc<dyn CognitionComponent>,
        decision: DecisionService,
        action_execution: ActionExecutionService,
    ) -> Self {
        Self {
            cognition,
            decision,
            action_execution,
        }
    }

    pub(crate) async fn run(
        &self,
        input: &ThoughtProcessInput,
    ) -> Result<ThoughtProcessResult, ThoughtProcessError> {
        println!("THOUGHT_PROCESS stage=start events={}", input.events.len());
        let decision_context = self
            .cognition
            .build_decision_context(input)
            .await
            .map_err(ThoughtProcessError::Cognition)?;
        let decision_output = self
            .decision
            .decide(&decision_context)
            .await
            .map_err(ThoughtProcessError::Decision)?;
        let action_results = self
            .action_execution
            .execute(
                &decision_context.available_actions,
                &decision_output.actions,
            )
            .await;
        println!(
            "THOUGHT_PROCESS stage=end actions={} action_results={}",
            decision_output.actions.len(),
            action_results.len()
        );
        Ok(ThoughtProcessResult {
            decision_context,
            decision_output,
            action_results,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DecisionError {
    Llm(String),
    InvalidJson(String),
    UnavailableAction(String),
}

impl std::fmt::Display for DecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Llm(err) => write!(f, "decision llm failed: {}", err),
            Self::InvalidJson(err) => write!(f, "decision output must be valid JSON: {}", err),
            Self::UnavailableAction(name) => {
                write!(f, "decision selected unavailable action: {}", name)
            }
        }
    }
}

impl std::error::Error for DecisionError {}

pub(crate) struct DecisionService {
    llm: Arc<dyn LlmAdapter>,
}

pub(crate) struct EventSetCognition;

#[async_trait]
impl CognitionComponent for EventSetCognition {
    async fn build_decision_context(
        &self,
        input: &ThoughtProcessInput,
    ) -> Result<DecisionContext, String> {
        Ok(DecisionContext {
            context: format_event_lines(&input.events),
            available_actions: default_available_actions(),
        })
    }
}

pub(crate) async fn run_basic_thought_process(
    state: &AppState,
    events: Vec<Event>,
    runtime: &ModuleRuntime,
    base_instructions: &str,
    decision_instructions: &str,
) -> Result<ThoughtProcessResult, ThoughtProcessError> {
    let usage_recorder: Arc<dyn LlmUsageRecorder> =
        Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
    let decision = DecisionService::new(build_response_api_llm(ResponseApiConfig {
        model: runtime.model.clone(),
        instructions: build_decision_instructions(base_instructions, decision_instructions),
        temperature: runtime.temperature,
        max_output_tokens: runtime.max_output_tokens,
        tools: Vec::new(),
        tool_handler: None,
        usage_recorder: Some(usage_recorder),
        usage_context: Some(LlmUsageContext::new("user", "decision")),
        max_tool_rounds: 0,
    }));
    let state_for_reply = state.clone();
    let action_execution = ActionExecutionService::with_user_reply(Arc::new(move |event| {
        let state = state_for_reply.clone();
        tokio::task::block_in_place(|| {
            Handle::current().block_on(record_event(&state, event));
        });
    }));
    ThoughtProcessService::new(Arc::new(EventSetCognition), decision, action_execution)
        .run(&ThoughtProcessInput { events })
        .await
}

fn build_decision_instructions(base_instructions: &str, decision_instructions: &str) -> String {
    format!(
        "{}\n\n{}\n\n{}\n{}",
        base_instructions.trim(),
        decision_instructions.trim(),
        "You are the Decision component of the thought process.",
        "Return JSON only with shape {\"actions\":[{\"name\":\"...\",\"input\":\"...\"}],\"reason\":\"...\"}. Select only actions listed in the input. Use user_reply to send a message to the user."
    )
}

impl DecisionService {
    pub(crate) fn new(llm: Arc<dyn LlmAdapter>) -> Self {
        Self { llm }
    }

    pub(crate) async fn decide(
        &self,
        context: &DecisionContext,
    ) -> Result<DecisionOutput, DecisionError> {
        println!(
            "THOUGHT_DECISION stage=start context_len={} available_actions={}",
            context.context.len(),
            context.available_actions.len()
        );
        let response = self
            .llm
            .respond(LlmRequest {
                input: render_decision_input(context),
            })
            .await
            .map_err(|err| DecisionError::Llm(err.to_string()))?;
        let output = parse_decision_output(&response.text)?;
        validate_decision_actions(context, &output)?;
        println!(
            "THOUGHT_DECISION stage=end actions={} reason_len={}",
            output.actions.len(),
            output.reason.len()
        );
        Ok(output)
    }
}

fn render_decision_input(context: &DecisionContext) -> String {
    let actions = context
        .available_actions
        .iter()
        .map(|action| {
            format!(
                "- name: {}\n  description: {}\n  input: {}",
                action.name, action.description, action.input_description
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Context:\n{}\n\nAvailable actions:\n{}\n\nReturn JSON only with shape: {{\"actions\":[{{\"name\":\"...\",\"input\":\"...\"}}],\"reason\":\"...\"}}",
        context.context, actions
    )
}

fn parse_decision_output(raw: &str) -> Result<DecisionOutput, DecisionError> {
    serde_json::from_str::<DecisionOutput>(raw)
        .map_err(|err| DecisionError::InvalidJson(err.to_string()))
}

fn validate_decision_actions(
    context: &DecisionContext,
    output: &DecisionOutput,
) -> Result<(), DecisionError> {
    for action in &output.actions {
        if !context
            .available_actions
            .iter()
            .any(|available| available.name == action.name)
        {
            return Err(DecisionError::UnavailableAction(action.name.clone()));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionExecutionError {
    message: String,
}

impl std::fmt::Display for ActionExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ActionExecutionError {}

#[async_trait]
pub(crate) trait ActionExecutor: Send + Sync {
    async fn execute(&self, input: &str) -> Result<String, ActionExecutionError>;
}

pub(crate) struct UserReplyExecutor {
    emit_event: Arc<dyn Fn(Event) + Send + Sync>,
}

impl UserReplyExecutor {
    pub(crate) fn new(emit_event: Arc<dyn Fn(Event) + Send + Sync>) -> Self {
        Self { emit_event }
    }
}

#[async_trait]
impl ActionExecutor for UserReplyExecutor {
    async fn execute(&self, input: &str) -> Result<String, ActionExecutionError> {
        let event = response_text(input.to_string());
        (self.emit_event)(event);
        Ok("{\"ok\":true}".to_string())
    }
}

pub(crate) struct ActionExecutionService {
    executors: HashMap<String, Arc<dyn ActionExecutor>>,
}

impl ActionExecutionService {
    pub(crate) fn new(executors: HashMap<String, Arc<dyn ActionExecutor>>) -> Self {
        Self { executors }
    }

    pub(crate) fn with_user_reply(emit_event: Arc<dyn Fn(Event) + Send + Sync>) -> Self {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert(
            "user_reply".to_string(),
            Arc::new(UserReplyExecutor::new(emit_event)),
        );
        Self::new(executors)
    }

    pub(crate) async fn execute(
        &self,
        available_actions: &[AvailableAction],
        selected_actions: &[Action],
    ) -> Vec<ActionResult> {
        let mut results = Vec::with_capacity(selected_actions.len());
        for action in selected_actions {
            if !available_actions
                .iter()
                .any(|available| available.name == action.name)
            {
                let error = format!("unavailable action: {}", action.name);
                println!(
                    "THOUGHT_ACTION stage=validate name={} ok=false error={}",
                    action.name, error
                );
                results.push(ActionResult {
                    name: action.name.clone(),
                    ok: false,
                    output: String::new(),
                    error: Some(error),
                });
                continue;
            }
            let Some(executor) = self.executors.get(&action.name) else {
                let error = format!("missing executor for action: {}", action.name);
                println!(
                    "THOUGHT_ACTION stage=resolve name={} ok=false error={}",
                    action.name, error
                );
                results.push(ActionResult {
                    name: action.name.clone(),
                    ok: false,
                    output: String::new(),
                    error: Some(error),
                });
                continue;
            };
            println!(
                "THOUGHT_ACTION stage=execute name={} input_len={}",
                action.name,
                action.input.len()
            );
            match executor.execute(&action.input).await {
                Ok(output) => {
                    println!("THOUGHT_ACTION stage=end name={} ok=true", action.name);
                    results.push(ActionResult {
                        name: action.name.clone(),
                        ok: true,
                        output,
                        error: None,
                    });
                }
                Err(err) => {
                    let error = err.to_string();
                    println!(
                        "THOUGHT_ACTION stage=end name={} ok=false error={}",
                        action.name, error
                    );
                    results.push(ActionResult {
                        name: action.name.clone(),
                        ok: false,
                        output: String::new(),
                        error: Some(error),
                    });
                }
            }
        }
        results
    }
}

pub(crate) fn default_available_actions() -> Vec<AvailableAction> {
    vec![AvailableAction {
        name: "user_reply".to_string(),
        description: "Send a text message to the user.".to_string(),
        input_description: "The exact reply text.".to_string(),
    }]
}

#[allow(dead_code)]
fn _trace_payload(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or_else(|err| json!({ "error": err.to_string() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmError, LlmResponse};
    use std::sync::Mutex;

    struct StaticLlm {
        response: Result<String, String>,
        requests: Arc<Mutex<Vec<LlmRequest>>>,
    }

    #[async_trait]
    impl LlmAdapter for StaticLlm {
        async fn respond(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
            self.requests.lock().expect("lock").push(request);
            match &self.response {
                Ok(text) => Ok(LlmResponse {
                    text: text.clone(),
                    raw: json!({}),
                    tool_calls: Vec::new(),
                }),
                Err(err) => Err(LlmError::new(err.clone())),
            }
        }
    }

    struct FailingExecutor;

    #[async_trait]
    impl ActionExecutor for FailingExecutor {
        async fn execute(&self, _input: &str) -> Result<String, ActionExecutionError> {
            Err(ActionExecutionError {
                message: "boom".to_string(),
            })
        }
    }

    struct StaticCognition {
        context: Result<DecisionContext, String>,
        seen_event_count: Arc<Mutex<Vec<usize>>>,
    }

    #[async_trait]
    impl CognitionComponent for StaticCognition {
        async fn build_decision_context(
            &self,
            input: &ThoughtProcessInput,
        ) -> Result<DecisionContext, String> {
            self.seen_event_count
                .lock()
                .expect("lock")
                .push(input.events.len());
            self.context.clone()
        }
    }

    fn decision_context() -> DecisionContext {
        DecisionContext {
            context: "The user greeted Tsuki.".to_string(),
            available_actions: default_available_actions(),
        }
    }

    #[tokio::test]
    async fn decision_rejects_invalid_json_without_actions() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service = DecisionService::new(Arc::new(StaticLlm {
            response: Ok("decision=respond reason=test".to_string()),
            requests,
        }));

        let err = service
            .decide(&decision_context())
            .await
            .expect_err("must reject non-json");

        assert!(matches!(err, DecisionError::InvalidJson(_)));
    }

    #[tokio::test]
    async fn decision_rejects_action_not_in_available_actions() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service = DecisionService::new(Arc::new(StaticLlm {
            response: Ok(
                r#"{"actions":[{"name":"shell_exec","input":"date"}],"reason":"test"}"#.to_string(),
            ),
            requests,
        }));

        let err = service
            .decide(&decision_context())
            .await
            .expect_err("must reject unavailable action");

        assert_eq!(
            err,
            DecisionError::UnavailableAction("shell_exec".to_string())
        );
    }

    #[tokio::test]
    async fn decision_renders_context_and_available_actions_for_llm() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service = DecisionService::new(Arc::new(StaticLlm {
            response: Ok(
                r#"{"actions":[{"name":"user_reply","input":"hi"}],"reason":"test"}"#.to_string(),
            ),
            requests: requests.clone(),
        }));

        let output = service
            .decide(&decision_context())
            .await
            .expect("must decide");

        assert_eq!(output.actions[0].name, "user_reply");
        let requests = requests.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].input.contains("The user greeted Tsuki."));
        assert!(requests[0].input.contains("name: user_reply"));
        assert!(requests[0].input.contains("Return JSON only"));
    }

    #[tokio::test]
    async fn action_execution_emits_user_reply_event() {
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_executor = emitted.clone();
        let service = ActionExecutionService::with_user_reply(Arc::new(move |event| {
            emitted_for_executor.lock().expect("lock").push(event);
        }));

        let results = service
            .execute(
                &default_available_actions(),
                &[Action {
                    name: "user_reply".to_string(),
                    input: "hello".to_string(),
                }],
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        let emitted = emitted.lock().expect("lock");
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].source, "assistant");
        assert_eq!(emitted[0].payload["text"], "hello");
        assert!(emitted[0].meta.tags.iter().any(|tag| tag == "response"));
    }

    #[tokio::test]
    async fn action_execution_rejects_unavailable_action() {
        let service = ActionExecutionService::new(HashMap::new());

        let results = service
            .execute(
                &default_available_actions(),
                &[Action {
                    name: "perform_task".to_string(),
                    input: "inspect logs".to_string(),
                }],
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert_eq!(
            results[0].error.as_deref(),
            Some("unavailable action: perform_task")
        );
    }

    #[tokio::test]
    async fn action_execution_reports_executor_failure() {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert("user_reply".to_string(), Arc::new(FailingExecutor));
        let service = ActionExecutionService::new(executors);

        let results = service
            .execute(
                &default_available_actions(),
                &[Action {
                    name: "user_reply".to_string(),
                    input: "hello".to_string(),
                }],
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert_eq!(results[0].error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn thought_process_runs_cognition_decision_and_execution() {
        let seen_event_count = Arc::new(Mutex::new(Vec::new()));
        let cognition = Arc::new(StaticCognition {
            context: Ok(decision_context()),
            seen_event_count: seen_event_count.clone(),
        });
        let decision_requests = Arc::new(Mutex::new(Vec::new()));
        let decision = DecisionService::new(Arc::new(StaticLlm {
            response: Ok(
                r#"{"actions":[{"name":"user_reply","input":"hi"}],"reason":"greeting"}"#
                    .to_string(),
            ),
            requests: decision_requests,
        }));
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_executor = emitted.clone();
        let actions = ActionExecutionService::with_user_reply(Arc::new(move |event| {
            emitted_for_executor.lock().expect("lock").push(event);
        }));
        let service = ThoughtProcessService::new(cognition, decision, actions);
        let input = ThoughtProcessInput {
            events: vec![crate::event::contracts::input_text("user", "message", "hi")],
        };

        let result = service.run(&input).await.expect("must run thought process");

        assert_eq!(*seen_event_count.lock().expect("lock"), vec![1]);
        assert_eq!(result.decision_output.reason, "greeting");
        assert_eq!(result.action_results.len(), 1);
        assert!(result.action_results[0].ok);
        assert_eq!(emitted.lock().expect("lock").len(), 1);
    }

    #[tokio::test]
    async fn thought_process_stops_before_execution_when_decision_fails() {
        let seen_event_count = Arc::new(Mutex::new(Vec::new()));
        let cognition = Arc::new(StaticCognition {
            context: Ok(decision_context()),
            seen_event_count,
        });
        let decision = DecisionService::new(Arc::new(StaticLlm {
            response: Ok("not json".to_string()),
            requests: Arc::new(Mutex::new(Vec::new())),
        }));
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_executor = emitted.clone();
        let actions = ActionExecutionService::with_user_reply(Arc::new(move |event| {
            emitted_for_executor.lock().expect("lock").push(event);
        }));
        let service = ThoughtProcessService::new(cognition, decision, actions);
        let input = ThoughtProcessInput { events: Vec::new() };

        let err = service
            .run(&input)
            .await
            .expect_err("decision failure must stop process");

        assert!(matches!(
            err,
            ThoughtProcessError::Decision(DecisionError::InvalidJson(_))
        ));
        assert!(emitted.lock().expect("lock").is_empty());
    }
}
