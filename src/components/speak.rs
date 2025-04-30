use async_trait::async_trait;
use bytes::Bytes;
use hound;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Cursor;
use thiserror::Error;
use tracing::{debug, info};

use crate::common::{
    broadcast::{self, IdentifiedBroadcast},
    chat::Modality,
    events::{self, Event, EventComponent},
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("serde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("hound error: {0}")]
    Hound(#[from] hound::Error),
    #[error("broadcast error: {0}")]
    Broadcast(#[from] broadcast::Error),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioQuery {
    #[serde(rename = "accent_phrases")]
    accent_phrases: Value,
    speed_scale: f32,
    pitch_scale: f32,
    intonation_scale: f32,
    volume_scale: f32,
    pre_phoneme_length: f32,
    post_phoneme_length: f32,
    pause_length: Option<f32>,
    pause_length_scale: f32,
    output_sampling_rate: i16,
    output_stereo: bool,
    kana: String,
}

pub struct SpeechEngine {
    client: Client,
    endpoint: String,
    speaker: u16,
}

impl SpeechEngine {
    pub fn new(endpoint: &str, speaker: u16) -> Self {
        Self {
            client: Client::new(),
            endpoint: endpoint.to_string(),
            speaker,
        }
    }

    pub async fn query(&self, text: &str) -> Result<AudioQuery, Error> {
        let mut params = HashMap::new();
        let speaker = self.speaker.to_string();
        params.insert("speaker", speaker.as_ref());
        params.insert("text", text);

        let response = self
            .client
            .post(format!("{}/audio_query", self.endpoint))
            .query(&params)
            .send()
            .await?
            .text()
            .await?;

        debug!(query = response);
        Ok(serde_json::from_str(&response)?)
    }

    pub async fn synthesis(&self, query: AudioQuery) -> Result<Bytes, Error> {
        let response = self
            .client
            .post(format!(
                "{}/synthesis?speaker={}",
                self.endpoint, self.speaker
            ))
            .body(serde_json::to_vec(&query)?)
            .send()
            .await?
            .bytes()
            .await?;

        Ok(response)
    }

    async fn run_internal(
        &mut self,
        mut broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), Error> {
        info!("start speech");

        loop {
            let event = broadcast.recv().await?;
            match event {
                Event::AssistantMessage {
                    modality: Modality::Audio,
                    message,
                    usage: _,
                } => {
                    let mut query = self.query(&message).await?;
                    query.speed_scale = 1.1;
                    query.pitch_scale = -0.02;
                    let audio = self.synthesis(query).await?;
                    info!(message = message, audio_size = audio.len(), "synthesized");

                    let cursor = Cursor::new(audio);
                    let mut reader = hound::WavReader::new(cursor)?;

                    broadcast.send(Event::PlayAudio {
                        sample_rate: reader.spec().sample_rate,
                        audio: reader
                            .samples::<i16>()
                            .collect::<Result<Vec<i16>, hound::Error>>()?,
                    })?;
                }
                _ => {}
            }
        }
    }
}

#[async_trait]
impl EventComponent for SpeechEngine {
    async fn run(
        &mut self,
        broadcast: IdentifiedBroadcast<Event>,
    ) -> Result<(), crate::common::events::Error> {
        self.run_internal(broadcast.participate())
            .await
            .map_err(|e| events::Error::Component(format!("speech: {}", e)))
    }
}
