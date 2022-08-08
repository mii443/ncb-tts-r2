use std::{path::Path, fs::File, io::Write};

use async_trait::async_trait;
use serenity::prelude::Context;

use crate::{tts::instance::TTSInstance, data::TTSClientData};

use super::gcp_tts::structs::{synthesize_request::SynthesizeRequest, synthesis_input::SynthesisInput, audio_config::AudioConfig, voice_selection_params::VoiceSelectionParams};

/// Message trait that can be used to synthesize text to speech.
#[async_trait]
pub trait TTSMessage {

    /// Parse the message for synthesis.
    ///
    /// Example:
    /// ```rust
    /// let text = message.parse(instance, ctx).await;
    /// ```
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> String;

    /// Synthesize the message and returns the path to the audio file.
    ///
    /// Example:
    /// ```rust
    /// let path = message.synthesize(instance, ctx).await;
    /// ```
    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> String;
}

pub struct AnnounceMessage {
    pub message: String,
}

#[async_trait]
impl TTSMessage for AnnounceMessage {
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> String {
        instance.before_message = None;
        format!(r#"<speak>アナウンス<break time="200ms"/>{}</speak>"#, self.message)
    }

    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> String {
        let text = self.parse(instance, ctx).await;
        let data_read = ctx.data.read().await;
        let storage = data_read.get::<TTSClientData>().expect("Cannot get TTSClientStorage").clone();
        let mut storage = storage.lock().await;

        let audio = storage.synthesize(SynthesizeRequest {
            input: SynthesisInput {
                text: None,
                ssml: Some(text)
            },
            voice: VoiceSelectionParams {
                languageCode: String::from("ja-JP"),
                name: String::from("ja-JP-Wavenet-B"),
                ssmlGender: String::from("neutral")
            },
            audioConfig: AudioConfig {
                audioEncoding: String::from("mp3"),
                speakingRate: 1.2f32,
                pitch: 1.0f32
            }
        }).await.unwrap();

        let uuid = uuid::Uuid::new_v4().to_string();

        let root = option_env!("CARGO_MANIFEST_DIR").unwrap();
        let path = Path::new(root);
        let file_path = path.join("audio").join(format!("{}.mp3", uuid));

        let mut file = File::create(file_path.clone()).unwrap();
        file.write(&audio).unwrap();

        file_path.into_os_string().into_string().unwrap()
    }
}