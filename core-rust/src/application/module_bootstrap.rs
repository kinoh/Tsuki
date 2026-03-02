use std::sync::Arc;

use crate::{
    activation_concept_graph::ConceptGraphStore,
    config::Config,
    event::Event,
    module_registry::{ModuleRegistry, ModuleRegistryReader},
    prompts::PromptOverrides,
    scheduler::ScheduleStore,
    state::StateStore,
    tools::{concept_graph_tools, state_tools, StateToolHandler},
};

#[derive(Clone)]
pub(crate) struct Modules {
    pub(crate) registry: ModuleRegistry,
    pub(crate) runtime: ModuleRuntime,
}

#[derive(Clone)]
pub(crate) struct ModuleRuntime {
    pub(crate) base_instructions: String,
    pub(crate) model: String,
    pub(crate) temperature: Option<f32>,
    pub(crate) max_output_tokens: Option<u32>,
    pub(crate) tools: Vec<async_openai::types::responses::Tool>,
    pub(crate) tool_handler: Arc<dyn crate::llm::ToolHandler>,
    pub(crate) max_tool_rounds: usize,
}

pub(crate) fn build_modules(
    state_store: Arc<dyn StateStore>,
    concept_graph: Arc<dyn ConceptGraphStore>,
    schedule_store: Arc<ScheduleStore>,
    registry: ModuleRegistry,
    config: &Config,
    base_instructions: String,
    emit_event: Arc<dyn Fn(Event) + Send + Sync>,
) -> Modules {
    let temperature = if config.llm.temperature_enabled {
        Some(config.llm.temperature)
    } else {
        None
    };
    let max_output_tokens = Some(config.llm.max_output_tokens);
    let mut tools = state_tools();
    tools.extend(concept_graph_tools(false, true));
    let tool_handler = Arc::new(StateToolHandler::new(
        state_store,
        concept_graph,
        schedule_store,
        emit_event.clone(),
        emit_event,
    ));

    let runtime = ModuleRuntime {
        base_instructions,
        model: config.llm.model.clone(),
        temperature,
        max_output_tokens,
        tools,
        tool_handler,
        max_tool_rounds: config.llm.max_tool_rounds,
    };

    Modules { registry, runtime }
}

pub(crate) async fn sync_module_registry_from_prompts(
    registry: &ModuleRegistry,
    prompts: &PromptOverrides,
) -> Result<(), String> {
    let desired = prompts.submodules.clone();
    let active = registry
        .list_active()
        .await
        .map_err(|err| err.to_string())?;

    for module in active {
        if !desired.contains_key(module.name.as_str()) {
            registry
                .upsert(module.name.as_str(), module.instructions.as_str(), false)
                .await
                .map_err(|err| err.to_string())?;
        }
    }
    for (name, instructions) in desired {
        registry
            .upsert(name.as_str(), instructions.as_str(), true)
            .await
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}
