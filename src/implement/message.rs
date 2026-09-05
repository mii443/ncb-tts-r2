use async_trait::async_trait;
use serenity::{model::prelude::Message, prelude::Context};
use songbird::tracks::Track;

use crate::{
    data::UserData,
    database::dictionary::Dictionary,
    errors::{constants::*, NCBError, Result},
    implement::member_name::ReadName,
    tts::{
        gcp_tts::structs::{
            audio_config::AudioConfig, synthesis_input::SynthesisInput,
            synthesize_request::SynthesizeRequest,
        },
        instance::TTSInstance,
        message::TTSMessage,
        text::{bounded_text, SpeechText},
        tts_type::TTSType,
    },
    utils::get_cached_regex,
};

fn apply_dictionary(text: &str, dictionary: &Dictionary) -> String {
    let mut text = bounded_text(text);
    for rule in &dictionary.rules {
        let replaced = if rule.is_regex {
            match get_cached_regex(&rule.rule) {
                Ok(regex) => regex.replace_all(&text, rule.to.as_str()).into_owned(),
                Err(_) => {
                    tracing::warn!("Skipping invalid dictionary regex");
                    continue;
                }
            }
        } else {
            text.replace(&rule.rule, &rule.to)
        };
        // Bound every expansion, including the input passed to the next rule.
        text = bounded_text(&replaced);
    }
    text
}

#[async_trait]
impl TTSMessage for Message {
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> Result<SpeechText> {
        let config = ctx
            .data::<UserData>()
            .database
            .get_server_config_or_default(instance.guild.get())
            .await?
            .ok_or_else(|| NCBError::config("Server config not found"))?;
        let body = apply_dictionary(&self.content, &config.dictionary);
        let mut text = SpeechText::default();
        let same_author = instance
            .before_message
            .as_ref()
            .is_some_and(|before| before.author.id == self.author.id);
        if !same_author && config.read_username.unwrap_or(true) {
            let name = self
                .member
                .as_ref()
                .and_then(|member| member.nick.as_ref())
                .map(|nick| nick.to_string())
                .unwrap_or_else(|| self.author.read_name());
            text.push_text(&format!("{name}さんの発言"));
            text.pause();
        }
        text.push_text(&body);
        if !self.attachments.is_empty() {
            text.pause();
            text.push_text(&format!("{}個の添付ファイル", self.attachments.len()));
        }
        instance.before_message = Some(self.clone());
        Ok(text)
    }

    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Result<Vec<Track>> {
        let text = self.parse(instance, ctx).await?;
        if text.plain().trim().is_empty() {
            return Ok(vec![]);
        }
        let data = ctx.data::<UserData>();
        let config = data
            .database
            .get_user_config_or_default(self.author.id.get())
            .await?
            .ok_or_else(|| NCBError::config("User config not found"))?;
        let tts = &data.tts_client;
        let track = match config
            .tts_type
            .unwrap_or(TTSType::GCP)
            .available_or_default()
        {
            TTSType::GCP => {
                tts.synthesize_gcp(SynthesizeRequest {
                    input: SynthesisInput {
                        text: None,
                        ssml: Some(text.ssml()),
                    },
                    voice: config.gcp_tts_voice.unwrap_or_else(|| {
                        crate::tts::gcp_tts::structs::voice_selection_params::VoiceSelectionParams {
                            languageCode: "ja-JP".into(),
                            name: "ja-JP-Wavenet-B".into(),
                            ssmlGender: "neutral".into(),
                        }
                    }),
                    audioConfig: AudioConfig {
                        audioEncoding: "mp3".into(),
                        speakingRate: DEFAULT_SPEAKING_RATE,
                        pitch: DEFAULT_PITCH,
                    },
                })
                .await?
            }
            TTSType::VOICEVOX => {
                tts.synthesize_voicevox(
                    &text.plain(),
                    config.voicevox_speaker.unwrap_or(DEFAULT_VOICEVOX_SPEAKER),
                )
                .await?
            }
            #[cfg(toriel_voice)]
            TTSType::TORIEL => tts.synthesize_toriel(&text.plain()).await?,
            #[cfg(not(toriel_voice))]
            TTSType::TORIEL => unreachable!("unavailable engines fall back to GCP"),
        };
        Ok(vec![track])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::dictionary::Rule;

    #[test]
    fn dictionary_expansion_is_bounded_after_each_rule() {
        let dictionary = Dictionary {
            rules: vec![
                Rule {
                    id: "expand".into(),
                    is_regex: false,
                    rule: "あ".into(),
                    to: "🙂".repeat(500),
                },
                Rule {
                    id: "again".into(),
                    is_regex: true,
                    rule: "🙂".into(),
                    to: "あ".repeat(500),
                },
            ],
        };
        let result = apply_dictionary(&"あ".repeat(167), &dictionary);
        assert_eq!(result, "あ".repeat(MAX_TTS_TEXT_LENGTH));
    }
}
