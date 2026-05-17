use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, collections::HashSet, sync::Arc, time::Instant};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};
use tokio::runtime::Handle;
use uuid::Uuid;

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
        contracts::{action_result, response_text, thought_process_component},
        Event,
    },
    input_ingress::{MediaAttachment, RouterInput},
    llm::{
        build_response_api_llm, LlmAdapter, LlmRequest, LlmResponse, LlmUsage, LlmUsageContext,
        LlmUsageRecorder, ResponseApiConfig,
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
    pub(crate) action_context: ActionExecutionContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActionExecutionContext {
    pub(crate) recent_event_history: String,
    pub(crate) recalled_history: String,
    pub(crate) latest_input: String,
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
    pub(crate) payload_description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionOutput {
    #[serde(default)]
    pub(crate) actions: Vec<Action>,
    #[serde(default)]
    pub(crate) intent_scores: Vec<IntentScore>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntentScore {
    pub(crate) source: String,
    pub(crate) relevance: u8,
    pub(crate) specificity: u8,
    pub(crate) conversational_fit: u8,
    pub(crate) grounding: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Action {
    pub(crate) name: String,
    pub(crate) payload: ActionPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ActionPayload {
    UserReply(UserReplySelection),
    PerformTask(PerformTaskPayload),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UserReplySelection {
    pub(crate) source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FocusPragmaticIntent {
    pub(crate) operation: FocusOperation,
    pub(crate) motive: PragmaticMotive,
    pub(crate) target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutableAction {
    pub(crate) name: String,
    pub(crate) payload: ExecutableActionPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ExecutableActionPayload {
    UserReply(FocusPragmaticIntent),
    PerformTask(PerformTaskPayload),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FocusOperation {
    Paraphrase,
    Switch,
    Add,
    TopicShift,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PragmaticMotive {
    Affiliation,
    SelfInterest,
    Play,
    Epistemic,
    Meta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PerformTaskPayload {
    pub(crate) task: String,
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
    #[serde(default)]
    pub(crate) timings: Vec<ComponentTiming>,
    #[serde(default)]
    pub(crate) llm_usages: Vec<ComponentLlmUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ComponentTrace {
    pub(crate) source: String,
    pub(crate) payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ComponentTiming {
    pub(crate) component_key: String,
    pub(crate) elapsed_ms: u128,
    pub(crate) ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ComponentLlmUsage {
    pub(crate) component_key: String,
    pub(crate) usage_stat_id: Option<String>,
    pub(crate) input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
    pub(crate) reasoning_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
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
pub(crate) struct CognitionRunResult {
    pub(crate) context: DecisionContext,
    pub(crate) timings: Vec<ComponentTiming>,
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
    ) -> Result<CognitionRunResult, String>;
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
    pub(crate) llm_usage: Option<ComponentLlmUsage>,
}

pub(crate) struct ThoughtProcessService {
    cognition: Arc<dyn CognitionComponent>,
    contributors: Vec<Arc<dyn DeliberationContributor>>,
    decision: DecisionService,
    action_execution: ActionExecutionService,
    emit_component_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
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
            emit_component_event: None,
        }
    }

    pub(crate) fn with_component_event_emitter(
        mut self,
        emit_component_event: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> Self {
        self.emit_component_event = Some(emit_component_event);
        self
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
        let run_id = Uuid::new_v4().to_string();
        let total_started = Instant::now();
        let mut timings = Vec::<ComponentTiming>::new();
        let mut llm_usages = Vec::<ComponentLlmUsage>::new();
        let emit_observation = self.emit_component_event.as_ref();

        let cognition_started = Instant::now();
        let cognition_run = match self.cognition.build_decision_context(input).await {
            Ok(run) => {
                let elapsed_ms = cognition_started.elapsed().as_millis();
                if let Some(emit_event) = emit_observation {
                    emit_component_observation(
                        emit_event,
                        &run_id,
                        ComponentObservation {
                            component: "cognition",
                            input: Some(json!({ "events": _trace_payload(&input.events) })),
                            output: Some(_trace_payload(&run.context)),
                            elapsed_ms,
                            metrics: Some(_trace_payload(&run.timings)),
                            usage: None,
                            error: None,
                        },
                    );
                }
                run
            }
            Err(err) => {
                let elapsed_ms = cognition_started.elapsed().as_millis();
                if let Some(emit_event) = emit_observation {
                    emit_component_observation(
                        emit_event,
                        &run_id,
                        ComponentObservation {
                            component: "cognition",
                            input: Some(json!({ "events": _trace_payload(&input.events) })),
                            output: None,
                            elapsed_ms,
                            metrics: None,
                            usage: None,
                            error: Some(err.clone()),
                        },
                    );
                }
                return Err(ThoughtProcessError::Cognition(err));
            }
        };
        timings.push(component_timing(
            "cognition",
            cognition_started.elapsed().as_millis(),
            true,
        ));
        timings.extend(cognition_run.timings.clone());
        let decision_context = cognition_run.context;

        let deliberation_result = run_deliberation_contributors(
            &self.contributors,
            &decision_context,
            emit_observation,
            &run_id,
        )
        .await
        .map_err(ThoughtProcessError::Deliberation)?;
        timings.extend(deliberation_result.timings);
        llm_usages.extend(deliberation_result.llm_usages);

        let decision_started = Instant::now();
        let decision_input =
            render_decision_input(&decision_context, &deliberation_result.contributions);
        let decision_result = match self
            .decision
            .decide(&decision_context, &deliberation_result.contributions)
            .await
        {
            Ok(result) => {
                let elapsed_ms = decision_started.elapsed().as_millis();
                if let Some(emit_event) = emit_observation {
                    emit_component_observation(
                        emit_event,
                        &run_id,
                        ComponentObservation {
                            component: "decision",
                            input: Some(json!({ "prompt": decision_input })),
                            output: Some(_trace_payload(&result.output)),
                            elapsed_ms,
                            metrics: None,
                            usage: result.llm_usage.clone(),
                            error: None,
                        },
                    );
                }
                result
            }
            Err(err) => {
                let elapsed_ms = decision_started.elapsed().as_millis();
                if let Some(emit_event) = emit_observation {
                    emit_component_observation(
                        emit_event,
                        &run_id,
                        ComponentObservation {
                            component: "decision",
                            input: Some(json!({ "prompt": decision_input })),
                            output: None,
                            elapsed_ms,
                            metrics: None,
                            usage: None,
                            error: Some(err.to_string()),
                        },
                    );
                }
                return Err(ThoughtProcessError::Decision(err));
            }
        };
        timings.push(component_timing(
            "decision",
            decision_started.elapsed().as_millis(),
            true,
        ));
        if let Some(usage) = decision_result.llm_usage {
            llm_usages.push(usage);
        }

        let executable_actions = resolve_executable_actions(
            &decision_result.output.actions,
            &deliberation_result.contributions,
        )
        .map_err(ThoughtProcessError::Decision)?;

        let action_run = self
            .action_execution
            .execute(
                &decision_context.available_actions,
                &executable_actions,
                &decision_context.action_context,
                mode,
                emit_observation,
                &run_id,
            )
            .await;
        timings.extend(action_run.timings);
        llm_usages.extend(action_run.llm_usages);
        timings.push(component_timing(
            "total",
            total_started.elapsed().as_millis(),
            true,
        ));
        println!(
            "THOUGHT_PROCESS stage=end actions={} action_results={}",
            decision_result.output.actions.len(),
            action_run.action_results.len()
        );
        Ok(ThoughtProcessResult {
            decision_context,
            deliberation_contributions: deliberation_result.contributions,
            decision_output: decision_result.output,
            action_results: action_run.action_results,
            trace: ThoughtProcessTrace {
                deliberation: deliberation_result.traces,
                timings,
                llm_usages,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DecisionError {
    Llm(String),
    InvalidJson(String),
    UnavailableAction(String),
    InvalidActionPayload(String),
    UnavailableIntentSource(String),
    InvalidIntentCandidate(String),
}

impl std::fmt::Display for DecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Llm(err) => write!(f, "decision llm failed: {}", err),
            Self::InvalidJson(err) => write!(f, "decision output must be valid JSON: {}", err),
            Self::UnavailableAction(name) => {
                write!(f, "decision selected unavailable action: {}", name)
            }
            Self::InvalidActionPayload(err) => write!(f, "invalid action payload: {}", err),
            Self::UnavailableIntentSource(source) => {
                write!(f, "decision selected unavailable intent source: {}", source)
            }
            Self::InvalidIntentCandidate(err) => write!(f, "invalid intent candidate: {}", err),
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
        let llm_usage = component_llm_usage(&format!("deliberation:{}", self.name), &response);
        let text = response.text.trim().to_string();
        println!(
            "THOUGHT_CONTRIBUTOR stage=end source={} text_len={}",
            self.name,
            text.len()
        );
        if text.is_empty() {
            Ok(DeliberationContributorResult {
                llm_usage,
                ..DeliberationContributorResult::default()
            })
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
                llm_usage,
            })
        }
    }
}

#[async_trait]
impl CognitionComponent for AppCognition {
    async fn build_decision_context(
        &self,
        input: &ThoughtProcessInput,
    ) -> Result<CognitionRunResult, String> {
        let mut timings = Vec::<ComponentTiming>::new();
        let latest_input = latest_router_input(&input.events);
        let input_text = latest_input
            .as_ref()
            .map(RouterInput::display_text)
            .unwrap_or_default();
        let history_started = Instant::now();
        let recent_event_history = format_event_lines(&input.events);
        timings.push(component_timing(
            "cognition:recent_event_history",
            history_started.elapsed().as_millis(),
            true,
        ));
        let mut context_parts = Vec::<String>::new();
        context_parts.push(format!(
            "<recent_event_history>\n{}\n</recent_event_history>",
            recent_event_history
        ));
        context_parts.push(format_time_context(&input.events));
        if let Some(router_input) = latest_input.as_ref() {
            let symbolization_started = Instant::now();
            let symbolization =
                symbolize(router_input, self.state.services.router_symbolizer.as_ref()).await;
            timings.push(component_timing(
                "cognition:symbolize",
                symbolization_started.elapsed().as_millis(),
                symbolization.error.is_none(),
            ));
            if let Some(err) = &symbolization.error {
                println!("COGNITION_SYMBOLIZE_ERROR error={}", err);
            }
            let concept_limit = self.state.config.router.query_terms_max.max(1);
            let active_state_limit = self.state.config.router.active_state_limit.max(1);
            let concept_task = async {
                let retrieval_started = Instant::now();
                let retrieval = retrieve_concepts(
                    &symbolization.text,
                    router_input,
                    concept_limit,
                    &self.state.config.router.multimodal_embedding,
                    self.state.services.activation_concept_graph.as_ref(),
                )
                .await;
                let retrieval_elapsed = retrieval_started.elapsed().as_millis();
                let activation_started = Instant::now();
                let activation = activate_concepts(
                    &retrieval.candidate_concepts,
                    active_state_limit,
                    self.state.services.activation_concept_graph.as_ref(),
                    self.dry_run,
                )
                .await;
                let activation_elapsed = activation_started.elapsed().as_millis();
                (retrieval, retrieval_elapsed, activation, activation_elapsed)
            };
            let contributors_task = async {
                let contributors_started = Instant::now();
                let result = list_active_deliberation_contributors(&self.state).await;
                (result, contributors_started.elapsed().as_millis())
            };
            let recall_task = async {
                let recall_started = Instant::now();
                let recalled_history =
                    format_recalled_event_history(&self.state, &input_text, &HashSet::new()).await;
                (recalled_history, recall_started.elapsed().as_millis())
            };
            let (
                (retrieval, retrieval_elapsed, activation, activation_elapsed),
                (deliberation_contributors, contributors_elapsed),
                (recalled_history, recall_elapsed),
            ) = tokio::join!(concept_task, contributors_task, recall_task);
            let deliberation_contributors = deliberation_contributors?;
            timings.push(component_timing(
                "cognition:concept_retrieval",
                retrieval_elapsed,
                retrieval.errors.is_empty(),
            ));
            timings.extend(retrieval.timings.iter().map(|metric| {
                component_timing(
                    format!("cognition:{}", metric.key),
                    metric.elapsed_ms,
                    metric.ok,
                )
            }));
            for err in &retrieval.errors {
                println!("COGNITION_CONCEPT_RETRIEVAL_ERROR error={}", err);
            }
            timings.push(component_timing(
                "cognition:concept_activation",
                activation_elapsed,
                activation.errors.is_empty(),
            ));
            timings.extend(activation.timings.iter().map(|metric| {
                component_timing(
                    format!("cognition:{}", metric.key),
                    metric.elapsed_ms,
                    metric.ok,
                )
            }));
            for err in &activation.errors {
                println!("COGNITION_CONCEPT_ACTIVATION_ERROR error={}", err);
            }
            timings.push(component_timing(
                "cognition:active_deliberation_contributors",
                contributors_elapsed,
                true,
            ));
            timings.push(component_timing(
                "cognition:conversation_recall",
                recall_elapsed,
                true,
            ));
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
            return Ok(CognitionRunResult {
                context: DecisionContext {
                    context: context_parts.join("\n\n"),
                    available_actions: default_available_actions(),
                    deliberation_contributors,
                    action_context: ActionExecutionContext {
                        recent_event_history,
                        recalled_history,
                        latest_input: input_text.trim().to_string(),
                    },
                },
                timings,
            });
        } else {
            context_parts.push("<latest_input>\nnone\n</latest_input>".to_string());
            context_parts.push(
                "<active_concepts_and_arousal>\nnone\n</active_concepts_and_arousal>".to_string(),
            );
            context_parts.push("<recalled_history>\nnone\n</recalled_history>".to_string());
        }
        Ok(CognitionRunResult {
            context: DecisionContext {
                context: context_parts.join("\n\n"),
                available_actions: default_available_actions(),
                deliberation_contributors: Vec::new(),
                action_context: ActionExecutionContext {
                    recent_event_history,
                    recalled_history: "none".to_string(),
                    latest_input: "none".to_string(),
                },
            },
            timings,
        })
    }
}

pub(crate) async fn run_basic_thought_process(
    state: &AppState,
    events: Vec<Event>,
    runtime: &ModuleRuntime,
    base_instructions: &str,
    decision_instructions: &str,
    action_execution_instructions: &str,
) -> Result<ThoughtProcessResult, ThoughtProcessError> {
    run_basic_thought_process_with_mode(
        state,
        events,
        runtime,
        base_instructions,
        decision_instructions,
        action_execution_instructions,
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
    action_execution_instructions: &str,
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
    let action_execution = ActionExecutionService::with_default_executors(
        emit_event.clone(),
        runtime,
        state,
        action_execution_instructions,
    );
    let contributors = build_prompt_deliberation_contributors(state, runtime, base_instructions)
        .await
        .map_err(ThoughtProcessError::Deliberation)?;
    let service = ThoughtProcessService::new(
        Arc::new(AppCognition::new(state.clone(), false)),
        contributors,
        decision,
        action_execution,
    );
    let service = if mode == ThoughtProcessRunMode::Commit {
        service.with_component_event_emitter(emit_event)
    } else {
        service
    };
    service
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
        "Return JSON only with shape {\"actions\":[{\"name\":\"user_reply\",\"payload\":{\"source\":\"...\"}}],\"intent_scores\":[{\"source\":\"...\",\"relevance\":1,\"specificity\":1,\"conversational_fit\":1,\"grounding\":1}]}. Select only actions listed in the input. The actions array is an ordered action plan. Decision is responsible for listing every action needed to realize the selected intent; external work and user-visible speech are separate actions. perform_task carries out external work and does not substitute for user_reply. If the selected intent requires the user to receive an answer, completion notice, failure notice, or result summary, include a user_reply action in the ordered plan even when perform_task is also present. A later user_reply may use the results of earlier actions; keep its payload as {\"source\":\"...\"} and choose exactly one deliberation candidate by source. Use perform_task with payload {\"task\":\"...\"} only for complex external work that requires tools."
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
    ) -> Result<DecisionRunResult, DecisionError> {
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
        let llm_usage = component_llm_usage("decision", &response);
        println!(
            "THOUGHT_DECISION stage=end actions={} intent_scores={}",
            output.actions.len(),
            output.intent_scores.len()
        );
        Ok(DecisionRunResult { output, llm_usage })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecisionRunResult {
    pub(crate) output: DecisionOutput,
    pub(crate) llm_usage: Option<ComponentLlmUsage>,
}

impl std::ops::Deref for DecisionRunResult {
    type Target = DecisionOutput;

    fn deref(&self) -> &Self::Target {
        &self.output
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DeliberationRunResult {
    pub(crate) contributions: DeliberationContributions,
    pub(crate) traces: Vec<ComponentTrace>,
    pub(crate) timings: Vec<ComponentTiming>,
    pub(crate) llm_usages: Vec<ComponentLlmUsage>,
}

pub(crate) async fn run_deliberation_contributors(
    contributors: &[Arc<dyn DeliberationContributor>],
    context: &DecisionContext,
    emit_component_event: Option<&Arc<dyn Fn(Event) + Send + Sync>>,
    run_id: &str,
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
    let mut tasks = Vec::new();
    let mut executed = HashSet::<String>::new();
    for contributor in contributors {
        if !selected.contains(contributor.source()) {
            continue;
        }
        executed.insert(contributor.source().to_string());
        let component_key = format!("deliberation:{}", contributor.source());
        tasks.push(async move {
            let started = Instant::now();
            let output = contributor.contribute(context).await;
            let elapsed_ms = started.elapsed().as_millis();
            (component_key, output, elapsed_ms)
        });
    }
    let outputs = futures::future::join_all(tasks).await;
    let mut combined = DeliberationContributions::default();
    let mut traces = Vec::<ComponentTrace>::new();
    let mut timings = Vec::<ComponentTiming>::new();
    let mut llm_usages = Vec::<ComponentLlmUsage>::new();
    for (component_key, output, elapsed_ms) in outputs {
        let output = match output {
            Ok(output) => {
                timings.push(component_timing(&component_key, elapsed_ms, true));
                if let Some(emit_event) = emit_component_event {
                    let component = component_key.as_str();
                    let output_payload = json!({
                        "contributions": _trace_payload(&output.contributions),
                        "trace": output.trace.as_ref().map(_trace_payload),
                    });
                    emit_component_observation(
                        emit_event,
                        run_id,
                        ComponentObservation {
                            component,
                            input: Some(json!({ "decision_context": context.context.as_str() })),
                            output: Some(output_payload),
                            elapsed_ms,
                            metrics: None,
                            usage: output.llm_usage.clone(),
                            error: None,
                        },
                    );
                }
                output
            }
            Err(err) => {
                timings.push(component_timing(&component_key, elapsed_ms, false));
                if let Some(emit_event) = emit_component_event {
                    emit_component_observation(
                        emit_event,
                        run_id,
                        ComponentObservation {
                            component: component_key.as_str(),
                            input: Some(json!({ "decision_context": context.context.as_str() })),
                            output: None,
                            elapsed_ms,
                            metrics: None,
                            usage: None,
                            error: Some(err.clone()),
                        },
                    );
                }
                return Err(err);
            }
        };
        combined
            .intent_candidates
            .extend(output.contributions.intent_candidates);
        combined
            .constraints
            .extend(output.contributions.constraints);
        if let Some(trace) = output.trace {
            traces.push(trace);
        }
        if let Some(usage) = output.llm_usage {
            llm_usages.push(usage);
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
        timings,
        llm_usages,
    })
}

fn render_decision_input(
    context: &DecisionContext,
    contributions: &DeliberationContributions,
) -> String {
    format!(
        "Context:\n{}\n\nDeliberation contributions:\n{}\n\nConstraints:\n{}\n\nAvailable actions:\n{}\n\nReturn JSON only with shape: {{\"actions\":[{{\"name\":\"user_reply\",\"payload\":{{\"source\":\"...\"}}}}],\"intent_scores\":[{{\"source\":\"...\",\"relevance\":1,\"specificity\":1,\"conversational_fit\":1,\"grounding\":1}}]}}\nActions are ordered. Include all actions needed to realize the selected intent. perform_task does not send a user-visible reply; include a later user_reply when the user should receive the result, completion, failure, or summary. A later user_reply can use previous action results without adding fields to its payload.",
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
                "- name: {}\n  description: {}\n  payload: {}",
                action.name, action.description, action.payload_description
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
        validate_action_payload(action).map_err(DecisionError::InvalidActionPayload)?;
    }
    validate_intent_scores(&output.intent_scores).map_err(DecisionError::InvalidActionPayload)?;
    Ok(())
}

fn validate_intent_scores(scores: &[IntentScore]) -> Result<(), String> {
    for score in scores {
        if score.source.trim().is_empty() {
            return Err("intent_scores.source must not be empty".to_string());
        }
        for (field, value) in [
            ("relevance", score.relevance),
            ("specificity", score.specificity),
            ("conversational_fit", score.conversational_fit),
            ("grounding", score.grounding),
        ] {
            if !(1..=5).contains(&value) {
                return Err(format!(
                    "intent_scores.{} must be an integer from 1 to 5",
                    field
                ));
            }
        }
    }
    Ok(())
}

fn validate_action_payload(action: &Action) -> Result<(), String> {
    match (action.name.as_str(), &action.payload) {
        ("user_reply", ActionPayload::UserReply(selection)) => {
            if selection.source.trim().is_empty() {
                Err("user_reply payload.source must not be empty".to_string())
            } else {
                Ok(())
            }
        }
        ("perform_task", ActionPayload::PerformTask(payload)) => {
            if payload.task.trim().is_empty() {
                Err("perform_task payload.task must not be empty".to_string())
            } else {
                Ok(())
            }
        }
        ("user_reply", _) => Err("user_reply requires {\"source\":\"...\"} payload".to_string()),
        ("perform_task", _) => Err("perform_task requires {\"task\":\"...\"} payload".to_string()),
        _ => Ok(()),
    }
}

fn validate_focus_pragmatic_intent(intent: &FocusPragmaticIntent) -> Result<(), String> {
    if intent.target.trim().is_empty() {
        return Err("user_reply payload.target must not be empty".to_string());
    }
    Ok(())
}

fn resolve_executable_actions(
    actions: &[Action],
    contributions: &DeliberationContributions,
) -> Result<Vec<ExecutableAction>, DecisionError> {
    actions
        .iter()
        .map(|action| match &action.payload {
            ActionPayload::UserReply(selection) => {
                let candidate = contributions
                    .intent_candidates
                    .iter()
                    .find(|candidate| candidate.source == selection.source)
                    .ok_or_else(|| {
                        DecisionError::UnavailableIntentSource(selection.source.clone())
                    })?;
                let intent = parse_focus_pragmatic_candidate(candidate.text.as_str())
                    .map_err(DecisionError::InvalidIntentCandidate)?;
                Ok(ExecutableAction {
                    name: action.name.clone(),
                    payload: ExecutableActionPayload::UserReply(intent),
                })
            }
            ActionPayload::PerformTask(payload) => Ok(ExecutableAction {
                name: action.name.clone(),
                payload: ExecutableActionPayload::PerformTask(payload.clone()),
            }),
        })
        .collect()
}

fn parse_focus_pragmatic_candidate(raw: &str) -> Result<FocusPragmaticIntent, String> {
    let mut operation = None;
    let mut motive = None;
    let mut target = None;
    for part in raw.split(';') {
        let (key, value) = part
            .trim()
            .split_once('=')
            .ok_or_else(|| format!("intent candidate segment must contain '=': {}", part.trim()))?;
        let value = value.trim();
        match key.trim() {
            "operation" => operation = Some(parse_focus_operation(value)?),
            "motive" => motive = Some(parse_pragmatic_motive(value)?),
            "target" => target = Some(value.to_string()),
            other => return Err(format!("unknown intent candidate field: {}", other)),
        }
    }
    let intent = FocusPragmaticIntent {
        operation: operation.ok_or("intent candidate missing operation")?,
        motive: motive.ok_or("intent candidate missing motive")?,
        target: target.ok_or("intent candidate missing target")?,
    };
    validate_focus_pragmatic_intent(&intent)?;
    Ok(intent)
}

fn parse_focus_operation(raw: &str) -> Result<FocusOperation, String> {
    match raw {
        "paraphrase" => Ok(FocusOperation::Paraphrase),
        "switch" => Ok(FocusOperation::Switch),
        "add" => Ok(FocusOperation::Add),
        "topic_shift" => Ok(FocusOperation::TopicShift),
        _ => Err(format!("unknown focus operation: {}", raw)),
    }
}

fn parse_pragmatic_motive(raw: &str) -> Result<PragmaticMotive, String> {
    match raw {
        "affiliation" => Ok(PragmaticMotive::Affiliation),
        "self_interest" => Ok(PragmaticMotive::SelfInterest),
        "play" => Ok(PragmaticMotive::Play),
        "epistemic" => Ok(PragmaticMotive::Epistemic),
        "meta" => Ok(PragmaticMotive::Meta),
        _ => Err(format!("unknown pragmatic motive: {}", raw)),
    }
}

fn action_executor_input(
    action: &ExecutableAction,
    action_context: &ActionExecutionContext,
    prior_action_results: &[ActionResult],
) -> Result<String, ActionExecutionError> {
    match &action.payload {
        ExecutableActionPayload::UserReply(intent) => serde_json::to_string(&json!({
            "intent": intent,
            "recent_event_history": action_context.recent_event_history,
            "recalled_history": action_context.recalled_history,
            "latest_input": action_context.latest_input,
            "prior_action_results": prior_action_results,
        }))
        .map_err(|err| ActionExecutionError {
            message: err.to_string(),
        }),
        ExecutableActionPayload::PerformTask(payload) => serde_json::to_string(&json!({
            "task": payload.task,
            "recent_event_history": action_context.recent_event_history,
            "recalled_history": action_context.recalled_history,
            "latest_input": action_context.latest_input,
            "prior_action_results": prior_action_results,
        }))
        .map_err(|err| ActionExecutionError {
            message: err.to_string(),
        }),
    }
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
    async fn inspect(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError>;
    async fn commit(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ActionExecutorOutput {
    pub(crate) output: String,
    pub(crate) llm_usage: Option<ComponentLlmUsage>,
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
    async fn inspect(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
        self.realize(input).await
    }

    async fn commit(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
        let output = self.realize(input).await?;
        let event = response_text(output.output.clone());
        (self.emit_event)(event);
        Ok(output)
    }
}

impl UserReplyExecutor {
    async fn realize(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
        let response = self
            .llm
            .respond(LlmRequest {
                input: input.to_string(),
            })
            .await
            .map_err(|err| ActionExecutionError {
                message: err.to_string(),
            })?;
        let llm_usage = component_llm_usage("action_execution:user_reply", &response);
        Ok(ActionExecutorOutput {
            output: response.text,
            llm_usage,
        })
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
    async fn inspect(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
        Ok(ActionExecutorOutput {
            output: json!({
            "mode": "llm_mediated_task",
            "llm_input": input,
            "tools_available": false,
            "tools_available_in_commit": true,
            })
            .to_string(),
            llm_usage: None,
        })
    }

    async fn commit(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
        let response = self
            .llm
            .respond(LlmRequest {
                input: input.to_string(),
            })
            .await
            .map_err(|err| ActionExecutionError {
                message: err.to_string(),
            })?;
        let llm_usage = component_llm_usage("action_execution:perform_task", &response);
        Ok(ActionExecutorOutput {
            output: response.text,
            llm_usage,
        })
    }
}

pub(crate) struct ActionExecutionService {
    executors: HashMap<String, Arc<dyn ActionExecutor>>,
    emit_event: Option<Arc<dyn Fn(Event) + Send + Sync>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ActionExecutionRunResult {
    pub(crate) action_results: Vec<ActionResult>,
    pub(crate) timings: Vec<ComponentTiming>,
    pub(crate) llm_usages: Vec<ComponentLlmUsage>,
}

impl std::ops::Deref for ActionExecutionRunResult {
    type Target = Vec<ActionResult>;

    fn deref(&self) -> &Self::Target {
        &self.action_results
    }
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
        user_reply_instructions: &str,
    ) -> Self {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        let reply_usage_recorder: Arc<dyn LlmUsageRecorder> =
            Arc::new(DbLlmUsageRecorder::new(state.services.db.clone()));
        let reply_llm = build_response_api_llm(ResponseApiConfig {
            model: runtime.model.clone(),
            instructions: user_reply_instructions.trim().to_string(),
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
            instructions: "You are an execution component. You receive JSON containing task, recent_event_history, recalled_history, latest_input, and prior_action_results. Carry out the selected external action using available tools when needed, using the context fields as grounding for the task. Return a concise execution result for the action result log. Do not message the user directly.".to_string(),
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
        selected_actions: &[ExecutableAction],
        action_context: &ActionExecutionContext,
        mode: ThoughtProcessRunMode,
        emit_component_event: Option<&Arc<dyn Fn(Event) + Send + Sync>>,
        run_id: &str,
    ) -> ActionExecutionRunResult {
        let mut results = Vec::with_capacity(selected_actions.len());
        let mut timings = Vec::<ComponentTiming>::new();
        let mut llm_usages = Vec::<ComponentLlmUsage>::new();
        for action in selected_actions {
            let component_key = format!("action_execution:{}", action.name);
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
                    error: Some(error.clone()),
                };
                if mode == ThoughtProcessRunMode::Commit {
                    self.emit_action_result(&result);
                }
                timings.push(component_timing(&component_key, 0, false));
                if let Some(emit_event) = emit_component_event {
                    emit_component_observation(
                        emit_event,
                        run_id,
                        ComponentObservation {
                            component: component_key.as_str(),
                            input: Some(json!({
                                "action": _trace_payload(action),
                                "action_context": _trace_payload(action_context),
                            })),
                            output: None,
                            elapsed_ms: 0,
                            metrics: None,
                            usage: None,
                            error: Some(error.clone()),
                        },
                    );
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
                    error: Some(error.clone()),
                };
                if mode == ThoughtProcessRunMode::Commit {
                    self.emit_action_result(&result);
                }
                timings.push(component_timing(&component_key, 0, false));
                if let Some(emit_event) = emit_component_event {
                    emit_component_observation(
                        emit_event,
                        run_id,
                        ComponentObservation {
                            component: component_key.as_str(),
                            input: Some(json!({
                                "action": _trace_payload(action),
                                "action_context": _trace_payload(action_context),
                            })),
                            output: None,
                            elapsed_ms: 0,
                            metrics: None,
                            usage: None,
                            error: Some(error.clone()),
                        },
                    );
                }
                results.push(result);
                continue;
            };
            println!(
                "THOUGHT_ACTION stage=execute name={} payload_len={} mode={:?}",
                action.name,
                serde_json::to_string(&action.payload)
                    .map(|value| value.len())
                    .unwrap_or_default(),
                mode
            );
            let executor_input = match action_executor_input(action, action_context, &results) {
                Ok(input) => input,
                Err(err) => {
                    let error = err.to_string();
                    let result = ActionResult {
                        name: action.name.clone(),
                        ok: false,
                        output: String::new(),
                        error: Some(error.clone()),
                    };
                    if mode == ThoughtProcessRunMode::Commit {
                        self.emit_action_result(&result);
                    }
                    timings.push(component_timing(&component_key, 0, false));
                    if let Some(emit_event) = emit_component_event {
                        emit_component_observation(
                            emit_event,
                            run_id,
                            ComponentObservation {
                                component: component_key.as_str(),
                                input: Some(json!({
                                    "action": _trace_payload(action),
                                    "action_context": _trace_payload(action_context),
                                })),
                                output: None,
                                elapsed_ms: 0,
                                metrics: None,
                                usage: None,
                                error: Some(error.clone()),
                            },
                        );
                    }
                    results.push(result);
                    continue;
                }
            };
            let started = Instant::now();
            let execution = match mode {
                ThoughtProcessRunMode::DryRun => executor.inspect(&executor_input).await,
                ThoughtProcessRunMode::Commit => executor.commit(&executor_input).await,
            };
            let elapsed_ms = started.elapsed().as_millis();
            match execution {
                Ok(output) => {
                    println!("THOUGHT_ACTION stage=end name={} ok=true", action.name);
                    let result = ActionResult {
                        name: action.name.clone(),
                        ok: true,
                        output: output.output,
                        error: None,
                    };
                    if mode == ThoughtProcessRunMode::Commit {
                        self.emit_action_result(&result);
                    }
                    timings.push(component_timing(&component_key, elapsed_ms, true));
                    let llm_usage = output.llm_usage;
                    if let Some(usage) = llm_usage.clone() {
                        llm_usages.push(usage);
                    }
                    if let Some(emit_event) = emit_component_event {
                        emit_component_observation(
                            emit_event,
                            run_id,
                            ComponentObservation {
                                component: component_key.as_str(),
                                input: Some(json!({
                                    "action": _trace_payload(action),
                                    "action_context": _trace_payload(action_context),
                                })),
                                output: Some(_trace_payload(&result)),
                                elapsed_ms,
                                metrics: None,
                                usage: llm_usage,
                                error: None,
                            },
                        );
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
                        error: Some(error.clone()),
                    };
                    if mode == ThoughtProcessRunMode::Commit {
                        self.emit_action_result(&result);
                    }
                    timings.push(component_timing(&component_key, elapsed_ms, false));
                    if let Some(emit_event) = emit_component_event {
                        emit_component_observation(
                            emit_event,
                            run_id,
                            ComponentObservation {
                                component: component_key.as_str(),
                                input: Some(json!({
                                    "action": _trace_payload(action),
                                    "action_context": _trace_payload(action_context),
                                })),
                                output: None,
                                elapsed_ms,
                                metrics: None,
                                usage: None,
                                error: Some(error.clone()),
                            },
                        );
                    }
                    results.push(result);
                }
            }
        }
        ActionExecutionRunResult {
            action_results: results,
            timings,
            llm_usages,
        }
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
            payload_description: "{\"source\":\"deliberation contributor source\"}".to_string(),
        },
        AvailableAction {
            name: "perform_task".to_string(),
            description: "Carry out complex external work using the execution component and tools."
                .to_string(),
            payload_description: "{\"task\":\"concise task description\"}".to_string(),
        },
    ]
}

#[allow(dead_code)]
fn _trace_payload(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or_else(|err| json!({ "error": err.to_string() }))
}

struct ComponentObservation<'a> {
    component: &'a str,
    input: Option<Value>,
    output: Option<Value>,
    elapsed_ms: u128,
    metrics: Option<Value>,
    usage: Option<ComponentLlmUsage>,
    error: Option<String>,
}

fn emit_component_observation(
    emit_event: &Arc<dyn Fn(Event) + Send + Sync>,
    run_id: &str,
    observation: ComponentObservation<'_>,
) {
    let mut payload = serde_json::Map::new();
    if let Some(input) = observation.input {
        payload.insert("input".to_string(), input);
    }
    if let Some(output) = observation.output {
        payload.insert("output".to_string(), output);
    }
    payload.insert("elapsed_ms".to_string(), json!(observation.elapsed_ms));
    if let Some(metrics) = observation.metrics {
        payload.insert("metrics".to_string(), metrics);
    }
    if let Some(usage) = observation.usage {
        payload.insert("usage".to_string(), _trace_payload(&usage));
    }
    emit_event(thought_process_component(
        run_id,
        observation.component,
        payload,
        observation.error.as_deref(),
    ));
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

fn format_time_context(events: &[Event]) -> String {
    let now = OffsetDateTime::now_utc();
    let now_local = to_local_offset(now);
    let latest_user = events
        .iter()
        .rev()
        .find(|event| event.source == "user" && event.meta.tags.iter().any(|tag| tag == "input"))
        .and_then(|event| OffsetDateTime::parse(event.ts.as_str(), &Rfc3339).ok());
    let (latest_user_elapsed, date_changed) = latest_user
        .map(|ts| {
            let elapsed = now - ts;
            let latest_local = to_local_offset(ts);
            (
                format_duration(elapsed.whole_seconds().max(0)),
                latest_local.date() != now_local.date(),
            )
        })
        .unwrap_or_else(|| ("unknown".to_string(), false));
    format!(
        "<time_context>\ncurrent_time={}\nlatest_user_elapsed={}\ndate_changed_since_latest_user={}\n</time_context>",
        format_local_datetime(now_local),
        latest_user_elapsed,
        date_changed
    )
}

fn to_local_offset(value: OffsetDateTime) -> OffsetDateTime {
    UtcOffset::current_local_offset()
        .map(|offset| value.to_offset(offset))
        .unwrap_or(value)
}

fn format_local_datetime(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        value.month() as u8,
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    )
}

fn format_duration(total_seconds: i64) -> String {
    let days = total_seconds / 86_400;
    let hours = (total_seconds % 86_400) / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    if days > 0 {
        format!("{} days {} hours", days, hours)
    } else if hours > 0 {
        format!("{} hours {} minutes", hours, minutes)
    } else {
        format!("{} minutes", minutes)
    }
}

fn tool_name(tool: &async_openai::types::responses::Tool) -> Option<&str> {
    match tool {
        async_openai::types::responses::Tool::Function(def) => Some(def.name.as_str()),
        _ => None,
    }
}

fn component_timing(
    component_key: impl Into<String>,
    elapsed_ms: u128,
    ok: bool,
) -> ComponentTiming {
    ComponentTiming {
        component_key: component_key.into(),
        elapsed_ms,
        ok,
    }
}

fn component_llm_usage(component_key: &str, response: &LlmResponse) -> Option<ComponentLlmUsage> {
    response.usage.as_ref().map(|usage| {
        component_llm_usage_from_usage(component_key, response.usage_stat_id.clone(), usage)
    })
}

fn component_llm_usage_from_usage(
    component_key: &str,
    usage_stat_id: Option<String>,
    usage: &LlmUsage,
) -> ComponentLlmUsage {
    ComponentLlmUsage {
        component_key: component_key.to_string(),
        usage_stat_id,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        cached_input_tokens: usage.cached_input_tokens,
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
                    usage: None,
                    usage_stat_id: None,
                }),
                Err(err) => Err(LlmError::new(err.clone())),
            }
        }
    }

    struct FailingExecutor;

    #[async_trait]
    impl ActionExecutor for FailingExecutor {
        async fn inspect(
            &self,
            _input: &str,
        ) -> Result<ActionExecutorOutput, ActionExecutionError> {
            Err(ActionExecutionError {
                message: "boom".to_string(),
            })
        }

        async fn commit(&self, _input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
            Err(ActionExecutionError {
                message: "boom".to_string(),
            })
        }
    }

    struct EchoExecutor;

    #[async_trait]
    impl ActionExecutor for EchoExecutor {
        async fn inspect(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
            Ok(ActionExecutorOutput {
                output: json!({ "input": input }).to_string(),
                llm_usage: None,
            })
        }

        async fn commit(&self, input: &str) -> Result<ActionExecutorOutput, ActionExecutionError> {
            Ok(ActionExecutorOutput {
                output: format!("executed: {}", input),
                llm_usage: None,
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
        ) -> Result<CognitionRunResult, String> {
            self.seen_event_count
                .lock()
                .expect("lock")
                .push(input.events.len());
            self.context.clone().map(|context| CognitionRunResult {
                context,
                timings: vec![component_timing("cognition:test", 1, true)],
            })
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
            action_context: action_execution_context(),
        }
    }

    fn action_execution_context() -> ActionExecutionContext {
        ActionExecutionContext {
            recent_event_history: "ts | role | message\n2026-05-17 12:00:00 | user | hi"
                .to_string(),
            recalled_history: "none".to_string(),
            latest_input: "hi".to_string(),
        }
    }

    fn empty_contributions() -> DeliberationContributions {
        DeliberationContributions::default()
    }

    fn user_reply_action(target: &str) -> ExecutableAction {
        ExecutableAction {
            name: "user_reply".to_string(),
            payload: ExecutableActionPayload::UserReply(FocusPragmaticIntent {
                operation: FocusOperation::Add,
                motive: PragmaticMotive::Affiliation,
                target: target.to_string(),
            }),
        }
    }

    fn perform_task_action(task: &str) -> ExecutableAction {
        ExecutableAction {
            name: "perform_task".to_string(),
            payload: ExecutableActionPayload::PerformTask(PerformTaskPayload {
                task: task.to_string(),
            }),
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
                r#"{"actions":[{"name":"shell_exec","payload":{"task":"date"}}],"intent_scores":[]}"#
                    .to_string(),
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
    async fn decision_rejects_legacy_action_input_field() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service = DecisionService::new(Arc::new(StaticLlm {
            response: Ok(
                r#"{"actions":[{"name":"user_reply","input":"hi"}],"intent_scores":[]}"#
                    .to_string(),
            ),
            requests,
        }));

        let err = service
            .decide(&decision_context(), &empty_contributions())
            .await
            .expect_err("legacy input must be rejected");

        assert!(matches!(err, DecisionError::InvalidJson(_)));
    }

    #[tokio::test]
    async fn decision_renders_context_and_available_actions_for_llm() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let service = DecisionService::new(Arc::new(StaticLlm {
            response: Ok(
                r#"{"actions":[{"name":"user_reply","payload":{"source":"curiosity"}}],"intent_scores":[{"source":"curiosity","relevance":5,"specificity":5,"conversational_fit":5,"grounding":1}]}"#.to_string(),
            ),
            requests: requests.clone(),
        }));

        let output = service
            .decide(
                &decision_context(),
                &DeliberationContributions {
                    intent_candidates: vec![IntentCandidate {
                        source: "curiosity".to_string(),
                        text: "operation=add; motive=epistemic; target=greeting".to_string(),
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
        assert!(requests[0].input.contains("Actions are ordered"));
        assert!(requests[0].input.contains("perform_task does not send"));
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
                &[user_reply_action("greeting")],
                &action_execution_context(),
                ThoughtProcessRunMode::Commit,
                None,
                "test-run",
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
                &[ExecutableAction {
                    name: "shell_exec".to_string(),
                    payload: ExecutableActionPayload::PerformTask(PerformTaskPayload {
                        task: "inspect logs".to_string(),
                    }),
                }],
                &action_execution_context(),
                ThoughtProcessRunMode::Commit,
                None,
                "test-run",
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
                &[perform_task_action("inspect logs")],
                &action_execution_context(),
                ThoughtProcessRunMode::Commit,
                None,
                "test-run",
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
                &[perform_task_action("inspect logs")],
                &action_execution_context(),
                ThoughtProcessRunMode::Commit,
                None,
                "test-run",
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        assert!(results[0].output.contains("inspect logs"));
        assert!(results[0].output.contains("\"latest_input\":\"hi\""));
    }

    #[tokio::test]
    async fn action_execution_emits_component_observation_when_requested() {
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_observation = emitted.clone();
        let emit_observation: Arc<dyn Fn(Event) + Send + Sync> = Arc::new(move |event| {
            emitted_for_observation.lock().expect("lock").push(event);
        });
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert("perform_task".to_string(), Arc::new(EchoExecutor));
        let service = ActionExecutionService::new(executors);

        let results = service
            .execute(
                &default_available_actions(),
                &[perform_task_action("inspect logs")],
                &action_execution_context(),
                ThoughtProcessRunMode::Commit,
                Some(&emit_observation),
                "run-1",
            )
            .await;

        assert_eq!(results.len(), 1);
        let emitted = emitted.lock().expect("lock");
        assert_eq!(emitted.len(), 1);
        let event = &emitted[0];
        assert_eq!(event.source, "thought_process");
        assert_eq!(event.modality, "state");
        assert_eq!(event.payload["run_id"], "run-1");
        assert_eq!(event.payload["component"], "action_execution:perform_task");
        assert!(event.payload["output"]["output"]
            .as_str()
            .expect("output string")
            .contains("inspect logs"));
        assert!(event.payload.get("stage").is_none());
        assert!(event.payload.get("ok").is_none());
        assert!(event.payload.get("error").is_none());
        assert!(event.meta.tags.iter().any(|tag| tag == "debug"));
        assert!(event.meta.tags.iter().any(|tag| tag == "thought_process"));
    }

    #[tokio::test]
    async fn action_execution_reports_executor_failure() {
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert("user_reply".to_string(), Arc::new(FailingExecutor));
        let service = ActionExecutionService::new(executors);

        let results = service
            .execute(
                &default_available_actions(),
                &[user_reply_action("greeting")],
                &action_execution_context(),
                ThoughtProcessRunMode::Commit,
                None,
                "test-run",
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert_eq!(results[0].error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn action_execution_dry_run_realizes_user_reply_without_emitting() {
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
                &[user_reply_action("affiliation")],
                &action_execution_context(),
                ThoughtProcessRunMode::DryRun,
                None,
                "test-run",
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        assert_eq!(results[0].output, "hello surface");
        assert!(emitted.lock().expect("lock").is_empty());
        let reply_requests = reply_requests.lock().expect("lock");
        assert_eq!(reply_requests.len(), 1);
        assert!(reply_requests[0].input.contains("\"recent_event_history\""));
        assert!(reply_requests[0].input.contains("\"latest_input\":\"hi\""));
        assert!(reply_requests[0].input.contains("\"intent\""));
        assert!(reply_requests[0]
            .input
            .contains("\"prior_action_results\":[]"));
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
                &[perform_task_action("inspect logs")],
                &action_execution_context(),
                ThoughtProcessRunMode::DryRun,
                None,
                "test-run",
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].ok);
        assert!(results[0].output.contains("llm_mediated_task"));
        assert!(results[0].output.contains("tools_available_in_commit"));
        assert!(results[0]
            .output
            .contains("\\\"latest_input\\\":\\\"hi\\\""));
        assert!(task_requests.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn action_execution_passes_prior_results_to_later_user_reply() {
        let emitted = Arc::new(Mutex::new(Vec::<Event>::new()));
        let emitted_for_reply = emitted.clone();
        let reply_requests = Arc::new(Mutex::new(Vec::new()));
        let mut executors = HashMap::<String, Arc<dyn ActionExecutor>>::new();
        executors.insert("perform_task".to_string(), Arc::new(EchoExecutor));
        executors.insert(
            "user_reply".to_string(),
            Arc::new(UserReplyExecutor::new(
                Arc::new(move |event| {
                    emitted_for_reply.lock().expect("lock").push(event);
                }),
                Arc::new(StaticLlm {
                    response: Ok("task result surfaced".to_string()),
                    requests: reply_requests.clone(),
                }),
            )),
        );
        let service = ActionExecutionService::new(executors);

        let results = service
            .execute(
                &default_available_actions(),
                &[
                    perform_task_action("inspect logs"),
                    user_reply_action("report task result"),
                ],
                &action_execution_context(),
                ThoughtProcessRunMode::Commit,
                None,
                "test-run",
            )
            .await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.ok));
        let reply_requests = reply_requests.lock().expect("lock");
        assert_eq!(reply_requests.len(), 1);
        assert!(reply_requests[0].input.contains("\"prior_action_results\""));
        assert!(reply_requests[0]
            .input
            .contains("\"name\":\"perform_task\""));
        assert!(reply_requests[0].input.contains("executed:"));
        let emitted = emitted.lock().expect("lock");
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].source, "assistant");
        assert_eq!(emitted[0].payload["text"], "task result surfaced");
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
                r#"{"actions":[{"name":"user_reply","payload":{"source":"curiosity"}}],"intent_scores":[{"source":"curiosity","relevance":5,"specificity":5,"conversational_fit":5,"grounding":1}]}"#
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
                        text: "operation=add; motive=affiliation; target=greeting".to_string(),
                    }],
                    constraints: Vec::new(),
                },
                trace: Some(ComponentTrace {
                    source: "curiosity".to_string(),
                    payload: json!({"prompt": "rendered"}),
                }),
                llm_usage: None,
            }),
            seen_contexts: contributor_contexts.clone(),
        });
        let service = ThoughtProcessService::new(cognition, vec![contributor], decision, actions);
        let input = ThoughtProcessInput {
            events: vec![crate::event::contracts::input_text("user", "message", "hi")],
        };

        let result = service.run(&input).await.expect("must run thought process");

        assert_eq!(*seen_event_count.lock().expect("lock"), vec![1]);
        assert_eq!(result.decision_output.intent_scores.len(), 1);
        assert_eq!(result.deliberation_contributions.intent_candidates.len(), 1);
        assert_eq!(
            *contributor_contexts.lock().expect("lock"),
            vec!["The user greeted Tsuki.".to_string()]
        );
        assert_eq!(result.action_results.len(), 1);
        assert!(result.action_results[0].ok);
        assert_eq!(result.trace.deliberation.len(), 1);
        assert_eq!(result.trace.deliberation[0].source, "curiosity");
        assert!(result
            .trace
            .timings
            .iter()
            .any(|timing| timing.component_key == "cognition:test"));
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
                r#"{"actions":[{"name":"user_reply","payload":{"source":"curiosity"}}],"intent_scores":[{"source":"curiosity","relevance":5,"specificity":5,"conversational_fit":5,"grounding":1}]}"#
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
