#![feature(slice_pattern)]

mod adapter;
mod common;
mod components;

use clap::Parser;
use color_eyre::{eyre, Result};
use common::{
    events::EventSystem, message::ASSISTANT_NAME, mumble::MumbleClient, repository::Repository,
};
use components::{
    core::{DefinedMessage, Model, OpenAiCore},
    eventlogger::EventLogger,
    interactive::{InteractiveProxy, Signal},
    notifier::Notifier,
    recognizer::SpeechRecognizer,
    scheduler::Scheduler,
    speak::SpeechEngine,
    web::WebState,
};
use crossterm::event::{self, KeyCode};
use futures::future::select_all;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span},
    DefaultTerminal, Frame,
};
use serde::Serialize;
use std::{env, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{
    select,
    sync::{mpsc, RwLock},
    task::{yield_now, JoinHandle},
};
use tracing_subscriber::{filter, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Error, Debug)]
enum ApplicationError {
    #[error("component stopped: {0}")]
    ComponentStopped(usize),
    #[error("Numerical value conversion error: {0}")]
    TryFromInt(#[from] std::num::TryFromIntError),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("tokio join error: {0}")]
    TokioJoin(#[from] tokio::task::JoinError),
    #[error("Failed to send signal: {0}")]
    SendSignal(#[from] tokio::sync::mpsc::error::SendError<Signal>),
    #[error("eyre error: {0}")]
    EyreReport(#[from] eyre::Report),
    #[error("tui-logger error: {0}")]
    TuiLogger(#[from] tui_logger::TuiLoggerError),
    #[error("events error: {0}")]
    Events(#[from] common::events::Error),
    #[error("repository error: {0}")]
    Repository(#[from] common::repository::Error),
    #[error("mumble error: {0}")]
    Mumble(#[from] common::mumble::Error),
    #[error("recognizer error: {0}")]
    Recognizer(#[from] components::recognizer::Error),
    #[error("core error: {0}")]
    Core(#[from] components::core::Error),
    #[error("web error: {0}")]
    Web(#[from] components::web::Error),
    #[error("notifier error: {0}")]
    Notifier(#[from] components::notifier::Error),
    #[error("Scheduler error: {0}")]
    Scheduler(#[from] components::scheduler::Error),
    #[error("Invalid environment value: {0}")]
    EnvVar(&'static str),
}

#[cfg(debug_assertions)]
static_toml::static_toml! {
    static CONF = include_toml!("./conf/local.toml");
}
#[cfg(not(debug_assertions))]
static_toml::static_toml! {
    static CONF = include_toml!("./conf/default.toml");
}

#[derive(Parser, Debug, Serialize)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    audio: bool,
    #[arg(long)]
    interactive: bool,
    #[arg(long)]
    scheduler: bool,
    #[arg(long)]
    notifier: bool,
}

#[derive(Debug)]
struct InteractiveApp {
    signal_sender: mpsc::Sender<Signal>,
    is_waiting: Arc<RwLock<bool>>,
}

impl InteractiveApp {
    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<(), ApplicationError> {
        loop {
            terminal.draw(|frame| self.render(frame))?;
            if event::poll(Duration::from_millis(20))? {
                match event::read()? {
                    crossterm::event::Event::Key(key) => match key.code {
                        KeyCode::Esc => {
                            break Ok(());
                        }
                        KeyCode::Char('c') if *(self.is_waiting.read().await) => {
                            self.signal_sender.send(Signal::Continue).await?;
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            // Required to be cancelled by select!
            yield_now().await;
        }
    }

    fn render(&self, frame: &mut Frame) {
        let [logs, prompt] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

        frame.render_widget(tui_logger::TuiLoggerWidget::default(), logs);

        let mut line = Line::from(vec![
            Span::styled(
                "interactive",
                Style::new().fg(Color::Magenta).bg(Color::White),
            ),
            Span::raw(" [esc] exit"),
        ]);

        if self.is_waiting.try_read().map(|ptr| *ptr).unwrap_or(false) {
            line.push_span(" [c] continue");
        }

        frame.render_widget(line, prompt);
    }
}

async fn wait_components(
    futs: futures::future::SelectAll<JoinHandle<Result<(), common::events::Error>>>,
) -> Result<(), ApplicationError> {
    let (result, index, _) = futs.await;
    result??;
    Err(ApplicationError::ComponentStopped(index))
}

fn setup_logging(interactive: bool) -> Result<(), ApplicationError> {
    let filter = filter::Targets::new()
        .with_default(tracing::Level::WARN)
        .with_target("tsuki", tracing::Level::DEBUG);
    if interactive {
        tracing_subscriber::registry()
            .with(tui_logger::TuiTracingSubscriberLayer {})
            .with(filter)
            .init();
        tui_logger::init_logger(tui_logger::LevelFilter::Debug)?;
    } else {
        let fmt = tracing_subscriber::fmt::layer();
        tracing_subscriber::registry().with(filter).with(fmt).init();
    }
    Ok(())
}

fn get_envvar(name: &'static str) -> Result<String, ApplicationError> {
    match env::var(name) {
        Ok(v) if !v.is_empty() => Ok(v),
        _ => Err(ApplicationError::EnvVar(name)),
    }
}

async fn app() -> Result<(), ApplicationError> {
    let args = Args::parse();
    setup_logging(args.interactive)?;
    let args_json = serde_json::to_value(&args).unwrap();

    let mut event_system = EventSystem::new(32);

    let eventlogger = EventLogger::new();
    event_system.run(eventlogger);

    let pretty_history = cfg!(debug_assertions);
    let repository = Arc::new(RwLock::new(Repository::new(
        CONF.main.history_path,
        pretty_history,
    )?));

    if args.audio {
        let mumble_client = MumbleClient::new(
            CONF.recognizer.mumble_host,
            CONF.recognizer.mumble_port.try_into()?,
            ASSISTANT_NAME,
        )
        .await?;

        let speech_recognizer = SpeechRecognizer::new(
            mumble_client,
            CONF.recognizer.vosk_model_path,
            Duration::from_millis(100),
            Duration::from_millis(500),
        )?;

        event_system.run(speech_recognizer);

        let speaker = SpeechEngine::new(
            CONF.speak.voicevox_endpoint,
            CONF.speak.voicevox_speaker_index.try_into()?,
        );

        event_system.run(speaker);
    }

    if args.notifier {
        let notifier = Notifier::new().await?;
        event_system.run(notifier);
    }

    if args.scheduler {
        let mut scheduler = Scheduler::new(
            repository.clone(),
            Duration::from_secs(CONF.scheduler.resolution_secs.try_into()?),
        )
        .await?;
        if repository.read().await.schedules().is_empty() {
            scheduler
                .register(
                    String::from("0 0 19 * * *"),
                    DefinedMessage::FinishSession.to_string(),
                )
                .await?;
        }
        event_system.run(scheduler);
    }

    let model = if CONF.core.openai_model.is_empty() {
        Model::Echo
    } else {
        Model::OpenAi(CONF.core.openai_model.to_string())
    };
    let core = OpenAiCore::new(
        repository.clone(),
        &get_envvar("PROMPT_PRIVATE_KEY")?,
        model,
        &get_envvar("OPENAI_API_KEY")?,
        Some(CONF.core.dify_sandbox_host).filter(|h| !h.is_empty()),
        &get_envvar("DIFY_SANDBOX_API_KEY")?,
    )
    .await?;
    let mut interactive_app = if args.interactive {
        let (sender, receiver) = mpsc::channel(1);
        let core_interactive = InteractiveProxy::new(32, receiver, core);
        let is_waiting = core_interactive.watch();
        event_system.run(core_interactive);
        Some(InteractiveApp {
            signal_sender: sender,
            is_waiting,
        })
    } else {
        event_system.run(core);
        None
    };

    let web_interface = WebState::new(
        repository,
        CONF.web.port.try_into()?,
        &get_envvar("WEB_AUTH_TOKEN")?,
        args_json,
    )?;
    event_system.run(web_interface);

    let any_components = select_all(event_system.futures());

    if let Some(ref mut app) = interactive_app {
        color_eyre::install()?;
        let mut terminal = ratatui::init();
        let result = select! {
            r = app.run(&mut terminal) => r,
            r = wait_components(any_components) => r,
        };
        ratatui::restore();
        Ok(result?)
    } else {
        wait_components(any_components).await
    }
}

#[tokio::main]
async fn main() {
    match app().await {
        Ok(_) => (),
        Err(e) => panic!("Error: {}", e),
    }
}
