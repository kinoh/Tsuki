#![feature(slice_pattern)]

mod core;
mod events;
mod messages;
mod mumble;
mod recognizer;
mod speak;
mod web;

use clap::Parser;
use futures::future::select_all;
use std::time::Duration;
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
}

#[derive(Parser, Debug)]
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
    #[arg(long, default_value_t = 2953u16)]
    port: u16,
}

async fn app() -> Result<(), ApplicationError> {
    let args = Args::parse();

    let event_system = events::EventSystem::new(32);
    let mut futures = Vec::new();

    if !args.mumble_host.is_empty() && !args.vosk_model.is_empty() {
        let mumble_client = mumble::Client::new(
            args.mumble_host,
            args.mumble_port,
            core::ASSISTANT_NAME.to_string(),
        )
        .await?;

        let speech_recognizer = recognizer::SpeechRecognizer::new(
            mumble_client,
            args.vosk_model,
            Duration::from_millis(100),
            Duration::from_millis(500),
        )?;

        futures.push(event_system.run(speech_recognizer));
    }

    if !args.voicevox_endpoint.is_empty() {
        let speaker = speak::SpeechEngine::new(args.voicevox_endpoint, 58);

        futures.push(event_system.run(speaker));
    }

    let repository = RwLock::new(messages::MessageRepository::new(args.history)?);

    let model = if args.openai_model.is_empty() {
        core::Model::Echo
    } else {
        core::Model::OpenAi(args.openai_model)
    };
    let core = core::OpenAiCore::new(repository, model).await?;
    futures.push(event_system.run(core));

    let web_interface = web::WebState::new(args.port);
    futures.push(event_system.run(web_interface));

    let (result, index, _) = select_all(futures).await;

    result??;
    Err(ApplicationError::ComponentStopped(index))
}

#[tokio::main]
async fn main() {
    match app().await {
        Ok(_) => (),
        Err(e) => panic!("Error: {}", e),
    }
}
