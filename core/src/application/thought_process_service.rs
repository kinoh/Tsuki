use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, collections::HashSet, sync::Arc};
use tokio::runtime::Handle;

use crate::{
    app_state::AppState,
    application::{
        concept_activation_service::activate_concepts,
        concept_retrieval_service::retrieve_concepts,
        conversation_recall_service::format_recalled_event_history, event_service::record_event,
        history_service::format_event_lines, module_bootstrap::ModuleRuntime,
        router_symbolization_service::symbolize, usage_service::DbLlmUsageRecorder,
    },
    event::{
        contracts::{action_result, response_text},
        Event,
    },
    input_ingress::{MediaAttachment, RouterInput},
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

pub(crate) struct AppCognition {
    state: AppState,
    dry_run: bool,
}

impl AppCognition {
    pub(crate) fn new(state: AppState, dry_run: bool) -> Self {
        Self { state, dry_run }
    }
}

#[async_trait]
impl CognitionComponent for AppCognition {
    async fn build_decision_context(
        &self,
        input: &ThoughtProcessInput,
    ) -> Result<DecisionContext, String> {
        let latest_input = latest_router_input(&input.events);
        let input_text = latest_input
            .as_ref()
            .map(RouterInput::display_text)
            .unwrap_or_default();
        let recent_event_history = format_event_lines(&input.events);
        let mut context_parts = Vec::<String>::new();
        context_parts.push(format!(
            "<recent_event_history>\n{}\n</recent_event_history>",
            recent_event_history
        ));
        if let Some(router_input) = latest_input.as_ref() {
            let symbolization =
                symbolize(router_input, self.state.services.router_symbolizer.as_ref()).await;
            if let Some(err) = &symbolization.error {
                println!("COGNITION_SYMBOLIZE_ERROR error={}", err);
            }
            let concept_limit = self.state.config.router.query_terms_max.max(1);
            let active_state_limit = self.state.config.router.active_state_limit.max(1);
            let retrieval = retrieve_concepts(
                &symbolization.text,
                router_input,
                concept_limit,
                &self.state.config.router.multimodal_embedding,
                self.state.services.activation_concept_graph.as_ref(),
            )
            .await;
            for err in &retrieval.errors {
                println!("COGNITION_CONCEPT_RETRIEVAL_ERROR error={}", err);
            }
            let activation = activate_concepts(
                &retrieval.candidate_concepts,
                active_state_limit,
                self.state.services.activation_concept_graph.as_ref(),
                self.dry_run,
            )
            .await;
            for err in &activation.errors {
                println!("COGNITION_CONCEPT_ACTIVATION_ERROR error={}", err);
            }
            let recalled_history =
                format_recalled_event_history(&self.state, &input_text, &HashSet::new()).await;
            context_parts.push(format!(
                "<latest_input>\n{}\n</latest_input>",
                input_text.trim()
            ));
            context_parts.push(format!(
                "<symbolized_input>\n{}\n</symbolized_input>",
                symbolization.text.trim()
            ));
            context_parts.push(format!(
                "<concept_candidates source=\"{}\">\n{}\n</concept_candidates>",
                retrieval.candidate_source,
                format_list_or_none(&retrieval.candidate_concepts)
            ));
            context_parts.push(format!(
                "<active_concepts_and_arousal>\n{}\n</active_concepts_and_arousal>",
                activation.active_concepts_and_arousal
            ));
            context_parts.push(format!(
                "<recalled_history>\n{}\n</recalled_history>",
                recalled_history
            ));
        } else {
            context_parts.push("<latest_input>\nnone\n</latest_input>".to_string());
            context_parts.push(
                "<active_concepts_and_arousal>\nnone\n</active_concepts_and_arousal>".to_string(),
            );
            context_parts.push("<recalled_history>\nnone\n</recalled_history>".to_string());
        }
        Ok(DecisionContext {
            context: context_parts.join("\n\n"),
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
    let emit_event = emit_event_blocking(state.clone());
    let action_execution =
        ActionExecutionService::with_default_executors(emit_event, runtime, state);
    ThoughtProcessService::new(
        Arc::new(AppCognition::new(state.clone(), false)),
        decision,
        action_execution,
    )
    .run(&ThoughtProcessInput { events })
    .await
}

fn build_decision_instructions(base_instructions: &str, decision_instructions: &str) -> String {
    format!(
        "{}\n\n{}\n\n{}\n{}",
        base_instructions.trim(),
        decision_instructions.trim(),
        "You are the Decision component of the thought process.",
        "Return JSON only with shape {\"actions\":[{\"name\":\"...\",\"input\":\"...\"}],\"reason\":\"...\"}. Select only actions listed in the input. Use user_reply to send a message to the user. Use perform_task for complex external work that requires tools."
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

pub(crate) struct PerformTaskExecutor {
    llm: Arc<dyn LlmAdapter>,
}

impl PerformTaskExecutor {
    pub(crate) fn new(llm: Arc<dyn LlmAdapter>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl ActionExecutor for PerformTaskExecutor {
    async fn execute(&self, input: &str) -> Result<String, ActionExecutionError> {
        let response = self
            .llm
            .respond(LlmRequest {
                input: input.to_string(),
            })
            .await
            .map_err(|err| ActionExecutionError {
                message: err.to_string(),
            })?;
        Ok(response.text)
    }
}

pub(crate) struct ActionExecutionService {
    executors: HashMap<String, Arc<dyn ActionExecutor>>,
    emit_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
}

impl ActionExecutionService {
    #[cfg(test)]
    pub(crate) fn new(executors: HashMap<String, Arc<dyn ActionExecutor>>) -> Self {
        Self {
            executors,
            emit_event: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_user_reply(emit_event: Arc<dyn Fn(Event) + Send + Sync>) -> Self {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        let emit_event_for_reply = emit_event.clone();
        executors.insert(
            "user_reply".to_string(),
            Arc::new(UserReplyExecutor::new(emit_event_for_reply)),
        );
        Self {
            executors,
            emit_event: Some(emit_event),
        }
    }

    pub(crate) fn with_default_executors(
        emit_event: Arc<dyn Fn(Event) + Send + Sync>,
        runtime: &ModuleRuntime,
        state: &AppState,
    ) -> Self {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert(
            "user_reply".to_string(),
            Arc::new(UserReplyExecutor::new(emit_event.clone())),
        );
        let usage_recorder: Arc<dyn LlmUsageRecorder> =
            Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
        let task_tools = runtime
            .tools
            .iter()
            .filter(|tool| tool_name(tool) != Some("emit_user_reply"))
            .cloned()
            .collect::<Vec<_>>();
        let task_llm = build_response_api_llm(ResponseApiConfig {
            model: runtime.model.clone(),
            instructions: "You are an execution component. Carry out the selected external action using available tools when needed. Return a concise execution result for the action result log. Do not message the user directly.".to_string(),
            temperature: runtime.temperature,
            max_output_tokens: runtime.max_output_tokens,
            tools: task_tools,
            tool_handler: Some(runtime.tool_handler.clone()),
            usage_recorder: Some(usage_recorder),
            usage_context: Some(LlmUsageContext::new("user", "perform_task")),
            max_tool_rounds: runtime.max_tool_rounds,
        });
        executors.insert(
            "perform_task".to_string(),
            Arc::new(PerformTaskExecutor::new(task_llm)),
        );
        Self {
            executors,
            emit_event: Some(emit_event),
        }
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
                let result = ActionResult {
                    name: action.name.clone(),
                    ok: false,
                    output: String::new(),
                    error: Some(error),
                };
                self.emit_action_result(&result);
                results.push(result);
                continue;
            }
            let Some(executor) = self.executors.get(&action.name) else {
                let error = format!("missing executor for action: {}", action.name);
                println!(
                    "THOUGHT_ACTION stage=resolve name={} ok=false error={}",
                    action.name, error
                );
                let result = ActionResult {
                    name: action.name.clone(),
                    ok: false,
                    output: String::new(),
                    error: Some(error),
                };
                self.emit_action_result(&result);
                results.push(result);
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
                    let result = ActionResult {
                        name: action.name.clone(),
                        ok: true,
                        output,
                        error: None,
                    };
                    self.emit_action_result(&result);
                    results.push(result);
                }
                Err(err) => {
                    let error = err.to_string();
                    println!(
                        "THOUGHT_ACTION stage=end name={} ok=false error={}",
                        action.name, error
                    );
                    let result = ActionResult {
                        name: action.name.clone(),
                        ok: false,
                        output: String::new(),
                        error: Some(error),
                    };
                    self.emit_action_result(&result);
                    results.push(result);
                }
            }
        }
        results
    }

    fn emit_action_result(&self, result: &ActionResult) {
        if let Some(emit_event) = &self.emit_event {
            emit_event(action_result(
                &result.name,
                result.ok,
                &result.output,
                result.error.as_deref(),
            ));
        }
    }
}

pub(crate) fn default_available_actions() -> Vec<AvailableAction> {
    vec![
        AvailableAction {
            name: "user_reply".to_string(),
            description: "Send a text message to the user.".to_string(),
            input_description: "The exact reply text.".to_string(),
        },
        AvailableAction {
            name: "perform_task".to_string(),
            description: "Carry out complex external work using the execution component and tools."
                .to_string(),
            input_description: "A concise task description for the execution component."
                .to_string(),
        },
    ]
}

#[allow(dead_code)]
fn _trace_payload(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or_else(|err| json!({ "error": err.to_string() }))
}

fn emit_event_blocking(state: AppState) -> Arc<dyn Fn(Event) + Send + Sync> {
    Arc::new(move |event| {
        let state = state.clone();
        tokio::task::block_in_place(|| {
            Handle::current().block_on(record_event(&state, event));
        });
    })
}

fn latest_router_input(events: &[Event]) -> Option<RouterInput> {
    events.iter().rev().find_map(router_input_from_event)
}

fn router_input_from_event(event: &Event) -> Option<RouterInput> {
    if event.source != "user" || !event.meta.tags.iter().any(|tag| tag == "input") {
        return None;
    }
    let kind = event
        .meta
        .tags
        .iter()
        .find_map(|tag| tag.strip_prefix("type:"))
        .unwrap_or("message")
        .to_string();
    let text = event
        .payload
        .get("user_text")
        .or_else(|| event.payload.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let images = serde_json::from_value::<Vec<MediaAttachment>>(
        event
            .payload
            .get("images")
            .cloned()
            .unwrap_or_else(|| json!([])),
    )
    .unwrap_or_default();
    let audio = serde_json::from_value::<Vec<MediaAttachment>>(
        event
            .payload
            .get("audio")
            .cloned()
            .unwrap_or_else(|| json!([])),
    )
    .unwrap_or_default();
    Some(RouterInput::new(kind, text, images, audio))
}

fn format_list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join("\n")
    }
}

fn tool_name(tool: &async_openai::types::responses::Tool) -> Option<&str> {
    match tool {
        async_openai::types::responses::Tool::Function(def) => Some(def.name.as_str()),
        _ => None,
    }
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

    struct EchoExecutor;

    #[async_trait]
    impl ActionExecutor for EchoExecutor {
        async fn execute(&self, input: &str) -> Result<String, ActionExecutionError> {
            Ok(format!("executed: {}", input))
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
        assert!(requests[0].input.contains("name: perform_task"));
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
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0].source, "assistant");
        assert_eq!(emitted[0].payload["text"], "hello");
        assert!(emitted[0].meta.tags.iter().any(|tag| tag == "response"));
        assert_eq!(emitted[1].source, "action_execution");
        assert_eq!(emitted[1].payload["action"], "user_reply");
        assert_eq!(emitted[1].payload["ok"], true);
        assert!(emitted[1]
            .meta
            .tags
            .iter()
            .any(|tag| tag == "action.result"));
    }

    #[tokio::test]
    async fn action_execution_rejects_unavailable_action() {
        let service = ActionExecutionService::new(HashMap::new());

        let results = service
            .execute(
                &default_available_actions(),
                &[Action {
                    name: "shell_exec".to_string(),
                    input: "inspect logs".to_string(),
                }],
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert_eq!(
            results[0].error.as_deref(),
            Some("unavailable action: shell_exec")
        );
    }

    #[tokio::test]
    async fn action_execution_reports_missing_executor_for_available_action() {
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
            Some("missing executor for action: perform_task")
        );
    }

    #[tokio::test]
    async fn action_execution_delegates_perform_task_to_executor() {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert("perform_task".to_string(), Arc::new(EchoExecutor));
        let service = ActionExecutionService::new(executors);

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
        assert!(results[0].ok);
        assert_eq!(results[0].output, "executed: inspect logs");
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
        assert_eq!(emitted.lock().expect("lock").len(), 2);
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
