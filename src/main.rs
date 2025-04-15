#![feature(slice_pattern)]

mod core;
mod eventlogger;
mod events;
mod executor;
mod messages;
mod mumble;
mod notifier;
mod recognizer;
mod speak;
mod ticker;
mod web;

use clap::Parser;
use futures::future::select_all;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Error, Debug)]
enum ApplicationError {
    #[error("mumble error: {0}")]
    Mumble(#[from] mumble::Error),
    #[error("recognizer error: {0}")]
    Recognizer(#[from] recognizer::Error),
    #[error("repository error: {0}")]
    Repository(#[from] messages::Error),
    #[error("core error: {0}")]
    Core(#[from] core::Error),
    #[error("events error: {0}")]
    Events(#[from] events::Error),
    #[error("tokio join error: {0}")]
    TokioJoin(#[from] tokio::task::JoinError),
    #[error("component stopped: {0}")]
    ComponentStopped(usize),
    #[error("web error: {0}")]
    Web(#[from] web::Error),
    #[error("executor error: {0}")]
    Executor(#[from] executor::Error),
    #[error("notifier error: {0}")]
    Notifier(#[from] notifier::Error),
}

#[derive(Parser, Debug, Serialize)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "")]
    mumble_host: String,
    #[arg(long, default_value_t = 64738u16)]
    mumble_port: u16,
    #[arg(long, default_value = "")]
    vosk_model: String,
    #[arg(long)]
    history: String,
    #[arg(long, default_value = "")]
    openai_model: String,
    #[arg(long, default_value = "")]
    voicevox_endpoint: String,
    #[arg(long, default_value = "")]
    dify_host: String,
    #[arg(long, default_value_t = 2953u16)]
    port: u16,
    #[arg(long, default_value_t = 0u64)]
    tick_interval_mins: u64,
}

async fn app() -> Result<(), ApplicationError> {
    let args = Args::parse();
    let args_json = serde_json::to_value(&args).unwrap();

    let mut event_system = events::EventSystem::new(32);

    if !args.mumble_host.is_empty() && !args.vosk_model.is_empty() {
        let mumble_client = mumble::Client::new(
            args.mumble_host,
            args.mumble_port,
            messages::ASSISTANT_NAME.to_string(),
        )
        .await?;

        let speech_recognizer = recognizer::SpeechRecognizer::new(
            mumble_client,
            args.vosk_model,
            Duration::from_millis(100),
            Duration::from_millis(500),
        )?;

        event_system.run(speech_recognizer);
    }

    if !args.voicevox_endpoint.is_empty() {
        let speaker = speak::SpeechEngine::new(args.voicevox_endpoint, 58);

        event_system.run(speaker);
    }

    if !args.dify_host.is_empty() {
        let executor = executor::CodeExecutor::new(&args.dify_host)?;

        event_system.run(executor);
    }

    let notifier = notifier::Notifier::new().await?;
    event_system.run(notifier);

    let eventlogger = eventlogger::EventLogger::new();
    event_system.run(eventlogger);

    if args.tick_interval_mins > 0 {
        let ticker = ticker::Ticker::new(Duration::from_secs(args.tick_interval_mins * 60));
        event_system.run(ticker);
    }

    let repository = Arc::new(RwLock::new(messages::MessageRepository::new(args.history)?));

    let model = if args.openai_model.is_empty() {
        core::Model::Echo
    } else {
        core::Model::OpenAi(args.openai_model)
    };
    let core = core::OpenAiCore::new(repository.clone(), model).await?;
    event_system.run(core);

    let web_interface = web::WebState::new(repository, args.port, args_json)?;
    event_system.run(web_interface);

    let (result, index, _) = select_all(event_system.futures()).await;

    result??;
    Err(ApplicationError::ComponentStopped(index))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    match app().await {
        Ok(_) => (),
        Err(e) => panic!("Error: {}", e),
    }
}
