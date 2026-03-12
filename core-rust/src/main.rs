mod activation_concept_graph;
mod app_state;
mod application;
mod cli;
mod clock;
mod commands;
mod config;
mod conversation_recall_store;
mod db;
mod debug_api;
mod event;
mod event_store;
mod input_ingress;
mod llm;
mod mcp;
mod mcp_trigger_concepts;
mod module_registry;
mod multimodal_embedding;
mod notification;
mod prompts;
mod scheduler;
mod server_app;
mod state;
mod tools;

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
        cli::CliCommand::BackfillConversationRecall { limit } => {
            commands::backfill_conversation_recall::run(limit).await
        }
    }
}
