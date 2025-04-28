#![feature(slice_pattern)]

mod common;
mod components;

use clap::Parser;
use color_eyre::{eyre, Result};
use common::{
    events::EventSystem, message::ASSISTANT_NAME, mumble::MumbleClient, repository::Repository,
};
use components::{
    core::{Model, OpenAiCore},
    eventlogger::EventLogger,
    executor::CodeExecutor,
    interactive::{InteractiveProxy, Signal},
    notifier::Notifier,
    recognizer::SpeechRecognizer,
    speak::SpeechEngine,
    ticker::Ticker,
    web::WebState,
};
use crossterm::event::{self, Event, KeyCode};
use futures::future::select_all;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span},
    DefaultTerminal, Frame,
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
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
    #[arg(long)]
    interactive: bool,
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
                    Event::Key(key) => match key.code {
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

async fn app() -> Result<(), ApplicationError> {
    let args = Args::parse();
    setup_logging(args.interactive)?;
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

    let pretty_history = cfg!(debug_assertions);
    let repository = Arc::new(RwLock::new(Repository::new(args.history, pretty_history)?));

    let model = if args.openai_model.is_empty() {
        Model::Echo
    } else {
        Model::OpenAi(args.openai_model)
    };
    let core = OpenAiCore::new(repository.clone(), model).await?;
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

    let web_interface = WebState::new(repository, args.port, args_json)?;
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
