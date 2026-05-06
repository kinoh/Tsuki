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
    module_registry::ModuleRegistryReader,
};

#[derive(Debug, Clone)]
pub(crate) struct ThoughtProcessInput {
    pub(crate) events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DecisionContext {
    pub(crate) context: String,
    pub(crate) available_actions: Vec<AvailableAction>,
    pub(crate) deliberation_contributors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct DeliberationContributions {
    #[serde(default)]
    pub(crate) intent_candidates: Vec<IntentCandidate>,
    #[serde(default)]
    pub(crate) constraints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IntentCandidate {
    pub(crate) source: String,
    pub(crate) text: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActionResult {
    pub(crate) name: String,
    pub(crate) ok: bool,
    pub(crate) output: String,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct ThoughtProcessTrace {
    #[serde(default)]
    pub(crate) deliberation: Vec<ComponentTrace>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ComponentTrace {
    pub(crate) source: String,
    pub(crate) payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ThoughtProcessResult {
    pub(crate) decision_context: DecisionContext,
    pub(crate) deliberation_contributions: DeliberationContributions,
    pub(crate) decision_output: DecisionOutput,
    pub(crate) action_results: Vec<ActionResult>,
    pub(crate) trace: ThoughtProcessTrace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ThoughtProcessError {
    Cognition(String),
    Deliberation(String),
    Decision(DecisionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThoughtProcessRunMode {
    DryRun,
    Commit,
}

impl std::fmt::Display for ThoughtProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cognition(err) => write!(f, "cognition failed: {}", err),
            Self::Deliberation(err) => write!(f, "deliberation contributor failed: {}", err),
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

#[async_trait]
pub(crate) trait DeliberationContributor: Send + Sync {
    fn source(&self) -> &str;

    async fn contribute(
        &self,
        context: &DecisionContext,
    ) -> Result<DeliberationContributorResult, String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DeliberationContributorResult {
    pub(crate) contributions: DeliberationContributions,
    pub(crate) trace: Option<ComponentTrace>,
}

pub(crate) struct ThoughtProcessService {
    cognition: Arc<dyn CognitionComponent>,
    contributors: Vec<Arc<dyn DeliberationContributor>>,
    decision: DecisionService,
    action_execution: ActionExecutionService,
}

impl ThoughtProcessService {
    pub(crate) fn new(
        cognition: Arc<dyn CognitionComponent>,
        contributors: Vec<Arc<dyn DeliberationContributor>>,
        decision: DecisionService,
        action_execution: ActionExecutionService,
    ) -> Self {
        Self {
            cognition,
            contributors,
            decision,
            action_execution,
        }
    }

    pub(crate) async fn run(
        &self,
        input: &ThoughtProcessInput,
    ) -> Result<ThoughtProcessResult, ThoughtProcessError> {
        self.run_with_mode(input, ThoughtProcessRunMode::Commit)
            .await
    }

    pub(crate) async fn run_with_mode(
        &self,
        input: &ThoughtProcessInput,
        mode: ThoughtProcessRunMode,
    ) -> Result<ThoughtProcessResult, ThoughtProcessError> {
        println!("THOUGHT_PROCESS stage=start events={}", input.events.len());
        let decision_context = self
            .cognition
            .build_decision_context(input)
            .await
            .map_err(ThoughtProcessError::Cognition)?;
        let deliberation_result =
            run_deliberation_contributors(&self.contributors, &decision_context)
                .await
                .map_err(ThoughtProcessError::Deliberation)?;
        let decision_output = self
            .decision
            .decide(&decision_context, &deliberation_result.contributions)
            .await
            .map_err(ThoughtProcessError::Decision)?;
        let action_results = self
            .action_execution
            .execute(
                &decision_context.available_actions,
                &decision_output.actions,
                mode,
            )
            .await;
        println!(
            "THOUGHT_PROCESS stage=end actions={} action_results={}",
            decision_output.actions.len(),
            action_results.len()
        );
        Ok(ThoughtProcessResult {
            decision_context,
            deliberation_contributions: deliberation_result.contributions,
            decision_output,
            action_results,
            trace: ThoughtProcessTrace {
                deliberation: deliberation_result.traces,
            },
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

pub(crate) struct PromptDeliberationContributor {
    name: String,
    llm: Arc<dyn LlmAdapter>,
    context_template: String,
}

impl PromptDeliberationContributor {
    pub(crate) fn new(
        name: impl Into<String>,
        llm: Arc<dyn LlmAdapter>,
        context_template: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            llm,
            context_template: context_template.into(),
        }
    }
}

#[async_trait]
impl DeliberationContributor for PromptDeliberationContributor {
    fn source(&self) -> &str {
        &self.name
    }

    async fn contribute(
        &self,
        context: &DecisionContext,
    ) -> Result<DeliberationContributorResult, String> {
        let input = render_contributor_input(&self.context_template, context);
        println!(
            "THOUGHT_CONTRIBUTOR stage=start source={} input_len={}",
            self.name,
            input.len()
        );
        let response = self
            .llm
            .respond(LlmRequest { input })
            .await
            .map_err(|err| err.to_string())?;
        let text = response.text.trim().to_string();
        println!(
            "THOUGHT_CONTRIBUTOR stage=end source={} text_len={}",
            self.name,
            text.len()
        );
        if text.is_empty() {
            Ok(DeliberationContributorResult::default())
        } else {
            Ok(DeliberationContributorResult {
                contributions: DeliberationContributions {
                    intent_candidates: vec![IntentCandidate {
                        source: self.name.clone(),
                        text,
                    }],
                    constraints: Vec::new(),
                },
                trace: None,
            })
        }
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
            let deliberation_contributors =
                list_active_deliberation_contributors(&self.state).await?;
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
            context_parts.push(format!(
                "<deliberation_contributors>\n{}\n</deliberation_contributors>",
                format_list_or_none(&deliberation_contributors)
            ));
            return Ok(DecisionContext {
                context: context_parts.join("\n\n"),
                available_actions: default_available_actions(),
                deliberation_contributors,
            });
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
            deliberation_contributors: Vec::new(),
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
    run_basic_thought_process_with_mode(
        state,
        events,
        runtime,
        base_instructions,
        decision_instructions,
        ThoughtProcessRunMode::Commit,
    )
    .await
}

pub(crate) async fn run_basic_thought_process_with_mode(
    state: &AppState,
    events: Vec<Event>,
    runtime: &ModuleRuntime,
    base_instructions: &str,
    decision_instructions: &str,
    mode: ThoughtProcessRunMode,
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
    let contributors = build_prompt_deliberation_contributors(state, runtime, base_instructions)
        .await
        .map_err(ThoughtProcessError::Deliberation)?;
    ThoughtProcessService::new(
        Arc::new(AppCognition::new(state.clone(), false)),
        contributors,
        decision,
        action_execution,
    )
    .run_with_mode(&ThoughtProcessInput { events }, mode)
    .await
}

pub(crate) async fn build_prompt_deliberation_contributors(
    state: &AppState,
    runtime: &ModuleRuntime,
    base_instructions: &str,
) -> Result<Vec<Arc<dyn DeliberationContributor>>, String> {
    let modules = state
        .runtime
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| err.to_string())?;
    let mut contributors = Vec::<Arc<dyn DeliberationContributor>>::new();
    for module in modules {
        let usage_recorder: Arc<dyn LlmUsageRecorder> =
            Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
        let llm = build_response_api_llm(ResponseApiConfig {
            model: runtime.model.clone(),
            instructions: compose_prompt_sections(&[
                base_instructions,
                module.instructions.as_str(),
                state
                    .config
                    .internal_prompts
                    .deliberation_contributor_instructions
                    .as_str(),
            ]),
            temperature: runtime.temperature,
            max_output_tokens: runtime.max_output_tokens,
            tools: Vec::new(),
            tool_handler: None,
            usage_recorder: Some(usage_recorder),
            usage_context: Some(LlmUsageContext::new(
                "user",
                format!("deliberation:{}", module.name),
            )),
            max_tool_rounds: 0,
        });
        contributors.push(Arc::new(PromptDeliberationContributor::new(
            module.name,
            llm,
            state
                .config
                .internal_prompts
                .deliberation_contributor_context_template
                .clone(),
        )));
    }
    Ok(contributors)
}

pub(crate) fn build_decision_instructions(
    base_instructions: &str,
    decision_instructions: &str,
) -> String {
    format!(
        "{}\n\n{}\n\n{}\n{}",
        base_instructions.trim(),
        decision_instructions.trim(),
        "You are the Decision component of the thought process.",
        "Return JSON only with shape {\"actions\":[{\"name\":\"...\",\"input\":\"...\"}],\"reason\":\"...\"}. Select only actions listed in the input. Use user_reply for conversational responses, but pass an abstract response policy or realization request instead of final surface text. Use perform_task for complex external work that requires tools."
    )
}

fn compose_prompt_sections(sections: &[&str]) -> String {
    sections
        .iter()
        .map(|section| section.trim())
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

impl DecisionService {
    pub(crate) fn new(llm: Arc<dyn LlmAdapter>) -> Self {
        Self { llm }
    }

    pub(crate) async fn decide(
        &self,
        context: &DecisionContext,
        contributions: &DeliberationContributions,
    ) -> Result<DecisionOutput, DecisionError> {
        println!(
            "THOUGHT_DECISION stage=start context_len={} available_actions={} intent_candidates={} constraints={}",
            context.context.len(),
            context.available_actions.len(),
            contributions.intent_candidates.len(),
            contributions.constraints.len()
        );
        let response = self
            .llm
            .respond(LlmRequest {
                input: render_decision_input(context, contributions),
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DeliberationRunResult {
    pub(crate) contributions: DeliberationContributions,
    pub(crate) traces: Vec<ComponentTrace>,
}

pub(crate) async fn run_deliberation_contributors(
    contributors: &[Arc<dyn DeliberationContributor>],
    context: &DecisionContext,
) -> Result<DeliberationRunResult, String> {
    let selected = context
        .deliberation_contributors
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    println!(
        "THOUGHT_CONTRIBUTIONS stage=start contributors={} selected={}",
        contributors.len(),
        selected.len()
    );
    let mut combined = DeliberationContributions::default();
    let mut traces = Vec::<ComponentTrace>::new();
    let mut executed = HashSet::<String>::new();
    for contributor in contributors {
        if !selected.contains(contributor.source()) {
            continue;
        }
        executed.insert(contributor.source().to_string());
        let output = contributor.contribute(context).await?;
        combined
            .intent_candidates
            .extend(output.contributions.intent_candidates);
        combined
            .constraints
            .extend(output.contributions.constraints);
        if let Some(trace) = output.trace {
            traces.push(trace);
        }
    }
    let missing = selected
        .iter()
        .filter(|source| !executed.contains(**source))
        .map(|source| source.to_string())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "selected deliberation contributors are unavailable: {}",
            missing.join(", ")
        ));
    }
    println!(
        "THOUGHT_CONTRIBUTIONS stage=end intent_candidates={} constraints={} traces={}",
        combined.intent_candidates.len(),
        combined.constraints.len(),
        traces.len()
    );
    Ok(DeliberationRunResult {
        contributions: combined,
        traces,
    })
}

fn render_decision_input(
    context: &DecisionContext,
    contributions: &DeliberationContributions,
) -> String {
    format!(
        "Context:\n{}\n\nDeliberation contributions:\n{}\n\nConstraints:\n{}\n\nAvailable actions:\n{}\n\nReturn JSON only with shape: {{\"actions\":[{{\"name\":\"...\",\"input\":\"...\"}}],\"reason\":\"...\"}}",
        context.context,
        format_intent_candidates(&contributions.intent_candidates),
        format_constraints(&contributions.constraints),
        format_available_actions(&context.available_actions)
    )
}

fn render_contributor_input(template: &str, context: &DecisionContext) -> String {
    template
        .replace("{{decision_context}}", &context.context)
        .replace(
            "{{available_actions}}",
            &format_available_actions(&context.available_actions),
        )
}

fn format_available_actions(actions: &[AvailableAction]) -> String {
    if actions.is_empty() {
        return "none".to_string();
    }
    actions
        .iter()
        .map(|action| {
            format!(
                "- name: {}\n  description: {}\n  input: {}",
                action.name, action.description, action.input_description
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_intent_candidates(candidates: &[IntentCandidate]) -> String {
    if candidates.is_empty() {
        return "none".to_string();
    }
    candidates
        .iter()
        .map(|candidate| format!("- {}: {}", candidate.source, candidate.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_constraints(constraints: &[String]) -> String {
    if constraints.is_empty() {
        return "none".to_string();
    }
    constraints
        .iter()
        .map(|constraint| format!("- {}", constraint))
        .collect::<Vec<_>>()
        .join("\n")
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
    async fn inspect(&self, input: &str) -> Result<Value, ActionExecutionError>;
    async fn commit(&self, input: &str) -> Result<String, ActionExecutionError>;
}

pub(crate) struct UserReplyExecutor {
    emit_event: Arc<dyn Fn(Event) + Send + Sync>,
    llm: Arc<dyn LlmAdapter>,
}

impl UserReplyExecutor {
    pub(crate) fn new(
        emit_event: Arc<dyn Fn(Event) + Send + Sync>,
        llm: Arc<dyn LlmAdapter>,
    ) -> Self {
        Self { emit_event, llm }
    }
}

#[async_trait]
impl ActionExecutor for UserReplyExecutor {
    async fn inspect(&self, input: &str) -> Result<Value, ActionExecutionError> {
        Ok(json!({
            "mode": "llm_mediated_user_reply",
            "llm_input": input,
            "tools_available": false,
        }))
    }

    async fn commit(&self, input: &str) -> Result<String, ActionExecutionError> {
        let response = self
            .llm
            .respond(LlmRequest {
                input: input.to_string(),
            })
            .await
            .map_err(|err| ActionExecutionError {
                message: err.to_string(),
            })?;
        let event = response_text(response.text.clone());
        (self.emit_event)(event);
        Ok(response.text)
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
    async fn inspect(&self, input: &str) -> Result<Value, ActionExecutionError> {
        Ok(json!({
            "mode": "llm_mediated_task",
            "llm_input": input,
            "tools_available": false,
            "tools_available_in_commit": true,
        }))
    }

    async fn commit(&self, input: &str) -> Result<String, ActionExecutionError> {
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
    pub(crate) fn with_user_reply(
        emit_event: Arc<dyn Fn(Event) + Send + Sync>,
        llm: Arc<dyn LlmAdapter>,
    ) -> Self {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        let emit_event_for_reply = emit_event.clone();
        executors.insert(
            "user_reply".to_string(),
            Arc::new(UserReplyExecutor::new(emit_event_for_reply, llm)),
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
        let reply_usage_recorder: Arc<dyn LlmUsageRecorder> =
            Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
        let reply_llm = build_response_api_llm(ResponseApiConfig {
            model: runtime.model.clone(),
            instructions: "You are the user_reply action executor. Realize the selected conversational intent as the final message to the user. Return only the message text. Do not call tools.".to_string(),
            temperature: runtime.temperature,
            max_output_tokens: runtime.max_output_tokens,
            tools: Vec::new(),
            tool_handler: None,
            usage_recorder: Some(reply_usage_recorder),
            usage_context: Some(LlmUsageContext::new("user", "user_reply")),
            max_tool_rounds: 0,
        });
        executors.insert(
            "user_reply".to_string(),
            Arc::new(UserReplyExecutor::new(emit_event.clone(), reply_llm)),
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
        mode: ThoughtProcessRunMode,
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
                if mode == ThoughtProcessRunMode::Commit {
                    self.emit_action_result(&result);
                }
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
                if mode == ThoughtProcessRunMode::Commit {
                    self.emit_action_result(&result);
                }
                results.push(result);
                continue;
            };
            println!(
                "THOUGHT_ACTION stage=execute name={} input_len={} mode={:?}",
                action.name,
                action.input.len(),
                mode
            );
            let execution = match mode {
                ThoughtProcessRunMode::DryRun => executor
                    .inspect(&action.input)
                    .await
                    .map(|value| value.to_string()),
                ThoughtProcessRunMode::Commit => executor.commit(&action.input).await,
            };
            match execution {
                Ok(output) => {
                    println!("THOUGHT_ACTION stage=end name={} ok=true", action.name);
                    let result = ActionResult {
                        name: action.name.clone(),
                        ok: true,
                        output,
                        error: None,
                    };
                    if mode == ThoughtProcessRunMode::Commit {
                        self.emit_action_result(&result);
                    }
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
                    if mode == ThoughtProcessRunMode::Commit {
                        self.emit_action_result(&result);
                    }
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
            description: "Realize a conversational response and send it to the user.".to_string(),
            input_description:
                "An abstract response policy or realization request, not the final surface text."
                    .to_string(),
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

pub(crate) fn emit_event_blocking(state: AppState) -> Arc<dyn Fn(Event) + Send + Sync> {
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

async fn list_active_deliberation_contributors(state: &AppState) -> Result<Vec<String>, String> {
    let mut active_modules = state
        .runtime
        .modules
        .registry
        .list_active()
        .await
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|module| module.name)
        .collect::<Vec<_>>();
    active_modules.sort();
    Ok(active_modules)
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
        async fn inspect(&self, _input: &str) -> Result<Value, ActionExecutionError> {
            Err(ActionExecutionError {
                message: "boom".to_string(),
            })
        }

        async fn commit(&self, _input: &str) -> Result<String, ActionExecutionError> {
            Err(ActionExecutionError {
                message: "boom".to_string(),
            })
        }
    }

    struct EchoExecutor;

    #[async_trait]
    impl ActionExecutor for EchoExecutor {
        async fn inspect(&self, input: &str) -> Result<Value, ActionExecutionError> {
            Ok(json!({ "input": input }))
        }

        async fn commit(&self, input: &str) -> Result<String, ActionExecutionError> {
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

    struct StaticContributor {
        output: Result<DeliberationContributorResult, String>,
        seen_contexts: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl DeliberationContributor for StaticContributor {
        fn source(&self) -> &str {
            "curiosity"
        }

        async fn contribute(
            &self,
            context: &DecisionContext,
        ) -> Result<DeliberationContributorResult, String> {
            self.seen_contexts
                .lock()
                .expect("lock")
                .push(context.context.clone());
            self.output.clone()
        }
    }

    fn decision_context() -> DecisionContext {
        DecisionContext {
            context: "The user greeted Tsuki.".to_string(),
            available_actions: default_available_actions(),
            deliberation_contributors: vec!["curiosity".to_string()],
        }
    }

    fn empty_contributions() -> DeliberationContributions {
        DeliberationContributions::default()
    }

    #[tokio::test]
    async fn decision_rejects_invalid_json_without_actions() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service = DecisionService::new(Arc::new(StaticLlm {
            response: Ok("decision=respond reason=test".to_string()),
            requests,
        }));

        let err = service
            .decide(&decision_context(), &empty_contributions())
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
            .decide(&decision_context(), &empty_contributions())
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
            .decide(
                &decision_context(),
                &DeliberationContributions {
                    intent_candidates: vec![IntentCandidate {
                        source: "curiosity".to_string(),
                        text: "operation=add; motive=epistemic".to_string(),
                    }],
                    constraints: vec!["do not execute external tasks".to_string()],
                },
            )
            .await
            .expect("must decide");

        assert_eq!(output.actions[0].name, "user_reply");
        let requests = requests.lock().expect("lock");
        assert_eq!(requests.len(), 1);
        assert!(requests[0].input.contains("The user greeted Tsuki."));
        assert!(requests[0]
            .input
            .contains("curiosity: operation=add; motive=epistemic"));
        assert!(requests[0].input.contains("do not execute external tasks"));
        assert!(requests[0].input.contains("name: user_reply"));
        assert!(requests[0].input.contains("name: perform_task"));
        assert!(requests[0].input.contains("Return JSON only"));
    }

    #[tokio::test]
    async fn action_execution_emits_user_reply_event() {
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_executor = emitted.clone();
        let service = ActionExecutionService::with_user_reply(
            Arc::new(move |event| {
                emitted_for_executor.lock().expect("lock").push(event);
            }),
            Arc::new(StaticLlm {
                response: Ok("hello surface".to_string()),
                requests: Arc::new(Mutex::new(Vec::new())),
            }),
        );

        let results = service
            .execute(
                &default_available_actions(),
                &[Action {
                    name: "user_reply".to_string(),
                    input: "hello".to_string(),
                }],
                ThoughtProcessRunMode::Commit,
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        let emitted = emitted.lock().expect("lock");
        assert_eq!(emitted.len(), 2);
        assert_eq!(emitted[0].source, "assistant");
        assert_eq!(emitted[0].payload["text"], "hello surface");
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
                ThoughtProcessRunMode::Commit,
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
                ThoughtProcessRunMode::Commit,
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
                ThoughtProcessRunMode::Commit,
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
                ThoughtProcessRunMode::Commit,
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert_eq!(results[0].error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn action_execution_dry_run_inspects_user_reply_without_emitting_or_calling_llm() {
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_executor = emitted.clone();
        let reply_requests = Arc::new(Mutex::new(Vec::new()));
        let service = ActionExecutionService::with_user_reply(
            Arc::new(move |event| {
                emitted_for_executor.lock().expect("lock").push(event);
            }),
            Arc::new(StaticLlm {
                response: Ok("hello surface".to_string()),
                requests: reply_requests.clone(),
            }),
        );

        let results = service
            .execute(
                &default_available_actions(),
                &[Action {
                    name: "user_reply".to_string(),
                    input: "operation=add; motive=affiliation".to_string(),
                }],
                ThoughtProcessRunMode::DryRun,
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        assert!(results[0].output.contains("llm_mediated_user_reply"));
        assert!(emitted.lock().expect("lock").is_empty());
        assert!(reply_requests.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn action_execution_dry_run_inspects_perform_task_without_calling_llm() {
        let task_requests = Arc::new(Mutex::new(Vec::new()));
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert(
            "perform_task".to_string(),
            Arc::new(PerformTaskExecutor::new(Arc::new(StaticLlm {
                response: Ok("done".to_string()),
                requests: task_requests.clone(),
            }))),
        );
        let service = ActionExecutionService::new(executors);

        let results = service
            .execute(
                &default_available_actions(),
                &[Action {
                    name: "perform_task".to_string(),
                    input: "inspect logs".to_string(),
                }],
                ThoughtProcessRunMode::DryRun,
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        assert!(results[0].output.contains("llm_mediated_task"));
        assert!(results[0].output.contains("tools_available_in_commit"));
        assert!(task_requests.lock().expect("lock").is_empty());
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
        let actions = ActionExecutionService::with_user_reply(
            Arc::new(move |event| {
                emitted_for_executor.lock().expect("lock").push(event);
            }),
            Arc::new(StaticLlm {
                response: Ok("hi surface".to_string()),
                requests: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        let contributor_contexts = Arc::new(Mutex::new(Vec::new()));
        let contributor = Arc::new(StaticContributor {
            output: Ok(DeliberationContributorResult {
                contributions: DeliberationContributions {
                    intent_candidates: vec![IntentCandidate {
                        source: "curiosity".to_string(),
                        text: "greet back lightly".to_string(),
                    }],
                    constraints: Vec::new(),
                },
                trace: Some(ComponentTrace {
                    source: "curiosity".to_string(),
                    payload: json!({"prompt": "rendered"}),
                }),
            }),
            seen_contexts: contributor_contexts.clone(),
        });
        let service = ThoughtProcessService::new(cognition, vec![contributor], decision, actions);
        let input = ThoughtProcessInput {
            events: vec![crate::event::contracts::input_text("user", "message", "hi")],
        };

        let result = service.run(&input).await.expect("must run thought process");

        assert_eq!(*seen_event_count.lock().expect("lock"), vec![1]);
        assert_eq!(result.decision_output.reason, "greeting");
        assert_eq!(result.deliberation_contributions.intent_candidates.len(), 1);
        assert_eq!(
            *contributor_contexts.lock().expect("lock"),
            vec!["The user greeted Tsuki.".to_string()]
        );
        assert_eq!(result.action_results.len(), 1);
        assert!(result.action_results[0].ok);
        assert_eq!(result.trace.deliberation.len(), 1);
        assert_eq!(result.trace.deliberation[0].source, "curiosity");
        assert_eq!(emitted.lock().expect("lock").len(), 2);
    }

    #[tokio::test]
    async fn thought_process_stops_before_execution_when_decision_fails() {
        let seen_event_count = Arc::new(Mutex::new(Vec::new()));
        let cognition = Arc::new(StaticCognition {
            context: Ok(DecisionContext {
                deliberation_contributors: Vec::new(),
                ..decision_context()
            }),
            seen_event_count,
        });
        let decision = DecisionService::new(Arc::new(StaticLlm {
            response: Ok("not json".to_string()),
            requests: Arc::new(Mutex::new(Vec::new())),
        }));
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_executor = emitted.clone();
        let actions = ActionExecutionService::with_user_reply(
            Arc::new(move |event| {
                emitted_for_executor.lock().expect("lock").push(event);
            }),
            Arc::new(StaticLlm {
                response: Ok("hi surface".to_string()),
                requests: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        let service = ThoughtProcessService::new(cognition, Vec::new(), decision, actions);
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

    #[tokio::test]
    async fn thought_process_stops_before_decision_when_contributor_fails() {
        let cognition = Arc::new(StaticCognition {
            context: Ok(decision_context()),
            seen_event_count: Arc::new(Mutex::new(Vec::new())),
        });
        let decision_requests = Arc::new(Mutex::new(Vec::new()));
        let decision = DecisionService::new(Arc::new(StaticLlm {
            response: Ok(
                r#"{"actions":[{"name":"user_reply","input":"hi"}],"reason":"greeting"}"#
                    .to_string(),
            ),
            requests: decision_requests.clone(),
        }));
        let contributor = Arc::new(StaticContributor {
            output: Err("cannot contribute".to_string()),
            seen_contexts: Arc::new(Mutex::new(Vec::new())),
        });
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_executor = emitted.clone();
        let actions = ActionExecutionService::with_user_reply(
            Arc::new(move |event| {
                emitted_for_executor.lock().expect("lock").push(event);
            }),
            Arc::new(StaticLlm {
                response: Ok("hi surface".to_string()),
                requests: Arc::new(Mutex::new(Vec::new())),
            }),
        );
        let service = ThoughtProcessService::new(cognition, vec![contributor], decision, actions);
        let input = ThoughtProcessInput { events: Vec::new() };

        let err = service
            .run(&input)
            .await
            .expect_err("contributor failure must stop process");

        assert!(matches!(err, ThoughtProcessError::Deliberation(_)));
        assert!(decision_requests.lock().expect("lock").is_empty());
        assert!(emitted.lock().expect("lock").is_empty());
    }
}
