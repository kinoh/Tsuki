#![feature(slice_pattern)]

mod core;
mod events;
mod messages;
mod mumble;
mod recognizer;

use clap::Parser;
use std::time::Duration;
use thiserror::Error;
use tokio::select;
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
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    mumble_host: String,
    #[arg(long, default_value_t = 64738u16)]
    mumble_port: u16,
    #[arg(long)]
    vosk_model: String,
    #[arg(long)]
    history: String,
    #[arg(long, default_value = "")]
    openai_model: String,
}

async fn app() -> Result<(), ApplicationError> {
    let args = Args::parse();

    let mumble_client = mumble::Client::new(
        args.mumble_host,
        args.mumble_port,
        true,
        "tsuki".to_string(),
    )
    .await?;

    let speech_recognizer = recognizer::SpeechRecognizer::new(
        mumble_client,
        args.vosk_model,
        Duration::from_millis(100),
        Duration::from_millis(500),
    )?;

    let repository = RwLock::new(messages::MessageRepository::new(args.history)?);

    let model = if args.openai_model.is_empty() {
        core::Model::Echo
    } else {
        core::Model::OpenAi(args.openai_model)
    };
    let core = core::OpenAiCore::new(repository, model)?;

    let event_system = events::EventSystem::new(32);

    select! {
        r = event_system.run(speech_recognizer).await => { println!("recognizer finished: {:?}", r); }
        r = event_system.run(core).await => { println!("core finished: {:?}", r); }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    match app().await {
        Ok(_) => (),
        Err(e) => panic!("Error: {}", e),
    }
}
