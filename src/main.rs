#![feature(slice_pattern)]

mod common;
mod components;

use clap::Parser;
use common::{
    events::EventSystem,
    messages::{MessageRepository, ASSISTANT_NAME},
    mumble::MumbleClient,
};
use components::{
    core::{Model, OpenAiCore},
    eventlogger::EventLogger,
    executor::CodeExecutor,
    notifier::Notifier,
    recognizer::SpeechRecognizer,
    speak::SpeechEngine,
    ticker::Ticker,
    web::WebState,
};
use futures::future::select_all;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Error, Debug)]
enum ApplicationError {
    #[error("component stopped: {0}")]
    ComponentStopped(usize),
    #[error("tokio join error: {0}")]
    TokioJoin(#[from] tokio::task::JoinError),
    #[error("events error: {0}")]
    Events(#[from] common::events::Error),
    #[error("repository error: {0}")]
    Repository(#[from] common::messages::Error),
    #[error("mumble error: {0}")]
    Mumble(#[from] common::mumble::Error),
    #[error("recognizer error: {0}")]
    Recognizer(#[from] components::recognizer::Error),
    #[error("core error: {0}")]
    Core(#[from] components::core::Error),
    #[error("web error: {0}")]
    Web(#[from] components::web::Error),
    #[error("executor error: {0}")]
    Executor(#[from] components::executor::Error),
    #[error("notifier error: {0}")]
    Notifier(#[from] components::notifier::Error),
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

    let mut event_system = EventSystem::new(32);

    if !args.mumble_host.is_empty() && !args.vosk_model.is_empty() {
        let mumble_client = MumbleClient::new(
            args.mumble_host,
            args.mumble_port,
            ASSISTANT_NAME.to_string(),
        )
        .await?;

        let speech_recognizer = SpeechRecognizer::new(
            mumble_client,
            args.vosk_model,
            Duration::from_millis(100),
            Duration::from_millis(500),
        )?;

        event_system.run(speech_recognizer);
    }

    if !args.voicevox_endpoint.is_empty() {
        let speaker = SpeechEngine::new(args.voicevox_endpoint, 58);

        event_system.run(speaker);
    }

    if !args.dify_host.is_empty() {
        let executor = CodeExecutor::new(&args.dify_host)?;

        event_system.run(executor);
    }

    let notifier = Notifier::new().await?;
    event_system.run(notifier);

    let eventlogger = EventLogger::new();
    event_system.run(eventlogger);

    if args.tick_interval_mins > 0 {
        let ticker = Ticker::new(Duration::from_secs(args.tick_interval_mins * 60));
        event_system.run(ticker);
    }

    let repository = Arc::new(RwLock::new(MessageRepository::new(args.history)?));

    let model = if args.openai_model.is_empty() {
        Model::Echo
    } else {
        Model::OpenAi(args.openai_model)
    };
    let core = OpenAiCore::new(repository.clone(), model).await?;
    event_system.run(core);

    let web_interface = WebState::new(repository, args.port, args_json)?;
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
