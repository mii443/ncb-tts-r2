use async_trait::async_trait;
use serenity::prelude::Context;
use songbird::tracks::Track;

use super::gcp_tts::structs::{
    audio_config::AudioConfig, synthesis_input::SynthesisInput,
    synthesize_request::SynthesizeRequest, voice_selection_params::VoiceSelectionParams,
};
use crate::{
    data::UserData,
    errors::Result,
    tts::{instance::TTSInstance, text::SpeechText},
};

#[async_trait]
pub trait TTSMessage: Send + Sync + std::fmt::Debug {
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> Result<SpeechText>;
    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Result<Vec<Track>>;
}

#[derive(Debug, Clone)]
pub struct AnnounceMessage {
    pub message: String,
}

#[async_trait]
impl TTSMessage for AnnounceMessage {
    async fn parse(&self, instance: &mut TTSInstance, _ctx: &Context) -> Result<SpeechText> {
        instance.before_message = None;
        let mut text = SpeechText::default();
        text.push_text("アナウンス");
        text.pause();
        text.push_text(&self.message);
        Ok(text)
    }

    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Result<Vec<Track>> {
        let text = self.parse(instance, ctx).await?;
        let audio = ctx
            .data::<UserData>()
            .tts_client
            .synthesize_gcp(SynthesizeRequest {
                input: SynthesisInput {
                    text: None,
                    ssml: Some(text.ssml()),
                },
                voice: VoiceSelectionParams {
                    languageCode: "ja-JP".into(),
                    name: "ja-JP-Wavenet-B".into(),
                    ssmlGender: "neutral".into(),
                },
                audioConfig: AudioConfig {
                    audioEncoding: "mp3".into(),
                    speakingRate: 1.2,
                    pitch: 1.0,
                },
            })
            .await?;
        Ok(vec![audio])
    }
}
