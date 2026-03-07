mod activation_concept_graph;
mod app_state;
mod application;
mod cli;
mod clock;
mod commands;
mod config;
mod db;
mod debug_api;
mod event;
mod event_store;
mod llm;
mod mcp;
mod mcp_trigger_concepts;
mod module_registry;
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
    }
}
