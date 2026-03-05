use async_trait::async_trait;
use serenity::prelude::Context;
use songbird::tracks::Track;

use crate::{data::UserData, tts::instance::TTSInstance};

use super::gcp_tts::structs::{
    audio_config::AudioConfig, synthesis_input::SynthesisInput,
    synthesize_request::SynthesizeRequest, voice_selection_params::VoiceSelectionParams,
};

/// Message trait that can be used to synthesize text to speech.
#[async_trait]
pub trait TTSMessage {
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> String;
    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Vec<Track>;
}

#[derive(Debug, Clone)]
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

    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Vec<Track> {
        let text = self.parse(instance, ctx).await;
        let data = ctx.data::<UserData>();
        let tts = &data.tts_client;

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

        vec![audio.into()]
    }
}
