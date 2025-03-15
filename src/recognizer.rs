use crate::events::{self, Event, EventComponent};
use crate::mumble::{self, Voice};
use async_trait::async_trait;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::task::JoinError;
use tokio::{select, sync::mpsc, time};
use voice_activity_detector::VoiceActivityDetector;
use vosk::{Model, Recognizer};

#[derive(Error, Debug)]
pub enum Error {
    #[error("vosk error: {0}")]
    Vosk(#[from] vosk::AcceptWaveformError),
    #[error("opus error: {0}")]
    Opus(#[from] opus::Error),
    #[error("system time error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),
    #[error("failed to send event: {0}")]
    SendText(#[from] broadcast::error::SendError<Event>),
    #[error("failed to receive event: {0}")]
    Receive(#[from] tokio::sync::broadcast::error::RecvError),
    #[error("failed to send voice: {0}")]
    SendVoice(#[from] tokio::sync::mpsc::error::SendError<Voice>),
    #[error("voice_activity_detector error: {0}")]
    Vad(#[from] voice_activity_detector::Error),
    #[error("join error: {0}")]
    Join(#[from] JoinError),
    #[error("mumble error: {0}")]
    Mumble(#[from] mumble::Error),
    #[error("failed to load model")]
    LoadModel,
    #[error("failed to create recognizer")]
    CreateRecognizer,
    #[error("duplicate run")]
    DuplicateRun,
    #[error("mumble client finished")]
    MumbleFinished,
}

const VAD_THREASHOLD: f32 = 0.7;
const VAD_REQUIRED_COUNT: u32 = 3;

pub struct BufferingVad {
    chunk_size: usize,
    vad: VoiceActivityDetector,
    buffer: Vec<i16>,
    count: u32,
}

impl BufferingVad {
    pub fn new(buffer_size: Duration) -> Result<Self, Error> {
        let sample_rate = mumble::SAMPLE_RATE;
        let chunk_size = (buffer_size.as_secs_f32() * (sample_rate as f32)) as usize;
        let vad = VoiceActivityDetector::builder()
            .sample_rate(sample_rate)
            .chunk_size(chunk_size)
            .build()?;
        Ok(BufferingVad {
            chunk_size,
            vad,
            buffer: Vec::new(),
            count: 0,
        })
    }

    pub fn detect(&mut self, audio: &[i16]) {
        self.buffer.extend_from_slice(audio);
        if self.buffer.len() >= self.chunk_size {
            let result = self.vad.predict(self.buffer.clone());
            if result > VAD_THREASHOLD {
                self.count += 1
            }
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.count = 0;
    }
}

pub struct SpeechRecognizer {
    mumble: Option<mumble::Client>,
    recognizer: Recognizer,
    monitor_interval: time::Duration,
    silence_timeout: time::Duration,
    buffering_vad: BufferingVad,
}

impl SpeechRecognizer {
    pub fn new(
        mumble: mumble::Client,
        vosk_model_path: String,
        monitor_interval: time::Duration,
        silence_timeout: time::Duration,
    ) -> Result<Self, Error> {
        let model = Model::new(vosk_model_path).ok_or(Error::LoadModel)?;
        let recognizer = Recognizer::new(&model, 48000f32).ok_or(Error::CreateRecognizer)?;
        let buffering_vad = BufferingVad::new(Duration::from_millis(500))?;
        Ok(Self {
            mumble: Some(mumble),
            recognizer,
            monitor_interval,
            silence_timeout,
            buffering_vad,
        })
    }

    async fn run_internal(
        &mut self,
        sender: Sender<Event>,
        mut receiver: Receiver<Event>,
    ) -> Result<(), Error> {
        let (hear_sender, mut hear_receiver) = mpsc::channel(32);
        let (speak_sender, mut speak_receiver) = mpsc::channel(32);

        let mut mumble_client = std::mem::take(&mut self.mumble).ok_or(Error::DuplicateRun)?;
        let mut mumble =
            tokio::spawn(async move { mumble_client.run(hear_sender, &mut speak_receiver).await });

        let mut last_receipt: Option<(String, SystemTime)> = None;
        let mut interval = time::interval(self.monitor_interval);

        loop {
            select! {
                Some(voice) = hear_receiver.recv() => {
                    self.buffering_vad.detect(&voice.audio);
                    let state = self.recognizer.accept_waveform(&voice.audio)?;

                    last_receipt = Some((voice.user.clone(), SystemTime::now()));

                    match state {
                        vosk::DecodingState::Failed => {
                            println!("recognition failed");
                        }
                        vosk::DecodingState::Finalized => {
                            if self.buffering_vad.count < VAD_REQUIRED_COUNT {
                                println!("vad count too few");
                                self.recognizer.reset();
                            } else {
                                let result = self.recognizer.result().single();
                                match result {
                                    None => println!("no result"),
                                    Some(value) => {
                                        println!("result: {:?} vad={}", value, self.buffering_vad.count);
                                        let text = value.text;
                                        if !text.is_empty() {
                                            sender.send(Event::RecognizedSpeech { user: voice.user, message: text.to_string() })?;
                                        }
                                    }
                                }
                            }
                        }
                        vosk::DecodingState::Running => {}
                    }
                }
                _ = interval.tick() => {
                    if let Some((user, t)) = last_receipt.take() {
                        let elapsed = SystemTime::now().duration_since(t)?;
                        if elapsed > self.silence_timeout {
                            if self.buffering_vad.count < VAD_REQUIRED_COUNT {
                                println!("vad count too few");
                                self.recognizer.reset();
                            } else {
                                let result = self.recognizer.final_result().single();
                                match result {
                                    None => println!("no final result"),
                                    Some(value) => {
                                        println!("final result: {:?} vad={}", value, self.buffering_vad.count);
                                        let text = value.text;
                                        if !text.is_empty() {
                                            sender.send(Event::RecognizedSpeech { user: user, message: text.to_string() })?;
                                        }
                                    }
                                }
                            }
                            last_receipt = None;
                            self.buffering_vad.clear();
                        } else {
                            last_receipt = Some((user, t));
                        }
                    }
                }
                event = receiver.recv() => {
                    match event? {
                        Event::PlayAudio { sample_rate, audio } => {
                            speak_sender.send(Voice { user: "".to_string(), sample_rate, audio }).await?;
                        }
                        _ => {}
                    }
                }
                result = &mut mumble => {
                    result??;
                    return Err(Error::MumbleFinished);
                }
            }
        }
    }
}

#[async_trait]
impl EventComponent for SpeechRecognizer {
    async fn run(&mut self, sender: Sender<Event>) -> Result<(), crate::events::Error> {
        let receiver = sender.subscribe();
        self.run_internal(sender, receiver)
            .await
            .map_err(|e| events::Error::Component(format!("recognizer: {}", e)))
    }
}
