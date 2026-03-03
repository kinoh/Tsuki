mod activation_concept_graph;
mod application;
mod cli;
mod clock;
mod commands;
mod config;
mod db;
mod event;
mod event_store;
mod llm;
mod module_registry;
mod notification;
mod prompts;
mod scheduler;
mod server_app;
mod state;
mod tools;

pub(crate) use server_app::{
    AppState, DebugImproveProposalRequest, DebugImproveResponse, DebugImproveReviewRequest,
    DebugRunRequest, DebugRunResponse, DebugTriggerRequest, DebugTriggerResponse,
};
pub(crate) use application::event_service::record_event;
pub(crate) use application::module_bootstrap::{ModuleRuntime, Modules};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("ERROR: {}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    match cli::parse_cli_command()? {
        cli::CliCommand::Serve => {
            server_app::run_server().await;
            Ok(())
        }
        cli::CliCommand::Backfill { limit } => commands::backfill::run(limit).await,
    }
}
