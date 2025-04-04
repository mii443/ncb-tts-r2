use async_trait::async_trait;
use serenity::prelude::Context;
use songbird::input::cached::Compressed;

use crate::{data::TTSClientData, tts::instance::TTSInstance};

use super::gcp_tts::structs::{
    audio_config::AudioConfig, synthesis_input::SynthesisInput,
    synthesize_request::SynthesizeRequest, voice_selection_params::VoiceSelectionParams,
};

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

    /// Synthesize the message and returns the audio data.
    ///
    /// Example:
    /// ```rust
    /// let audio = message.synthesize(instance, ctx).await;
    /// ```
    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Vec<Compressed>;
}

pub struct AnnounceMessage {
    pub message: String,
}

#[async_trait]
impl TTSMessage for AnnounceMessage {
    async fn parse(&self, instance: &mut TTSInstance, _ctx: &Context) -> String {
        instance.before_message = None;
        format!(
            r#"<speak>アナウンス<break time="200ms"/>{}</speak>"#,
            self.message
        )
    }

    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Vec<Compressed> {
        let text = self.parse(instance, ctx).await;
        let data_read = ctx.data.read().await;
        let tts = data_read
            .get::<TTSClientData>()
            .expect("Cannot get TTSClientStorage");

        let audio = tts
            .synthesize_gcp(SynthesizeRequest {
                input: SynthesisInput {
                    text: None,
                    ssml: Some(text),
                },
                voice: VoiceSelectionParams {
                    languageCode: String::from("ja-JP"),
                    name: String::from("ja-JP-Wavenet-B"),
                    ssmlGender: String::from("neutral"),
                },
                audioConfig: AudioConfig {
                    audioEncoding: String::from("mp3"),
                    speakingRate: 1.2f32,
                    pitch: 1.0f32,
                },
            })
            .await
            .unwrap();

        vec![audio]
    }
}
