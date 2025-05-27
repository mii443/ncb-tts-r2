use async_trait::async_trait;
use serenity::{model::prelude::Message, prelude::Context};
use songbird::tracks::Track;
use tracing::{error, warn};

use crate::{
    data::{DatabaseClientData, TTSClientData},
    errors::{constants::*, validation, NCBError},
    implement::member_name::ReadName,
    tts::{
        gcp_tts::structs::{
            audio_config::AudioConfig, synthesis_input::SynthesisInput,
            synthesize_request::SynthesizeRequest,
        },
        instance::TTSInstance,
        message::TTSMessage,
        tts_type::TTSType,
    },
    utils::{get_cached_regex, retry_with_backoff},
};

#[async_trait]
impl TTSMessage for Message {
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> String {
        let data_read = ctx.data.read().await;

        let config = {
            let database = data_read
                .get::<DatabaseClientData>()
                .ok_or_else(|| NCBError::config("Cannot get DatabaseClientData"))
                .map_err(|e| {
                    error!(error = %e, "Failed to get database client");
                    e
                })
                .unwrap(); // This is safe as we're in a critical path
            
            match database.get_server_config_or_default(instance.guild.get()).await {
                Ok(Some(config)) => config,
                Ok(None) => {
                    error!(guild_id = %instance.guild, "No server config available");
                    return self.content.clone(); // Fallback to original text
                },
                Err(e) => {
                    error!(guild_id = %instance.guild, error = %e, "Failed to get server config");
                    return self.content.clone(); // Fallback to original text
                }
            }
        };
        let mut text = self.content.clone();
        
        // Validate text length before processing
        if let Err(e) = validation::validate_tts_text(&text) {
            warn!(error = %e, "Invalid TTS text, using truncated version");
            text.truncate(crate::errors::constants::MAX_TTS_TEXT_LENGTH);
        }
        
        for rule in config.dictionary.rules {
            if rule.is_regex {
                match get_cached_regex(&rule.rule) {
                    Ok(regex) => {
                        text = regex.replace_all(&text, &rule.to).to_string();
                    }
                    Err(e) => {
                        warn!(
                            rule_id = rule.id,
                            pattern = rule.rule,
                            error = %e,
                            "Skipping invalid regex rule"
                        );
                        continue;
                    }
                }
            } else {
                text = text.replace(&rule.rule, &rule.to);
            }
        }
        let mut res = if let Some(before_message) = &instance.before_message {
            if before_message.author.id == self.author.id {
                text.clone()
            } else {
                let name = get_user_name(self, ctx).await;
                if config.read_username.unwrap_or(true) {
                    format!("{}さんの発言<break time=\"200ms\"/>{}", name, text)
                } else {
                    format!("{}", text)
                }
            }
        } else {
            let name = get_user_name(self, ctx).await;

            if config.read_username.unwrap_or(true) {
                format!("{}さんの発言<break time=\"200ms\"/>{}", name, text)
            } else {
                format!("{}", text)
            }
        };

        if self.attachments.len() > 0 {
            res = format!(
                "{}<break time=\"200ms\"/>{}個の添付ファイル",
                res,
                self.attachments.len()
            );
        }

        instance.before_message = Some(self.clone());

        res
    }

    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Vec<Track> {
        let text = self.parse(instance, ctx).await;

        let data_read = ctx.data.read().await;

        let config = {
            let database = data_read
                .get::<DatabaseClientData>()
                .ok_or_else(|| NCBError::config("Cannot get DatabaseClientData"))
                .unwrap();
            
            match database.get_user_config_or_default(self.author.id.get()).await {
                Ok(Some(config)) => config,
                Ok(None) | Err(_) => {
                    error!(user_id = %self.author.id, "Failed to get user config, using defaults");
                    // Return default config
                    crate::database::user_config::UserConfig {
                        tts_type: Some(TTSType::GCP),
                        gcp_tts_voice: Some(crate::tts::gcp_tts::structs::voice_selection_params::VoiceSelectionParams {
                            languageCode: String::from("ja-JP"),
                            name: String::from("ja-JP-Wavenet-B"),
                            ssmlGender: String::from("neutral"),
                        }),
                        voicevox_speaker: Some(crate::errors::constants::DEFAULT_VOICEVOX_SPEAKER),
                    }
                }
            }
        };

        let tts = data_read
            .get::<TTSClientData>()
            .ok_or_else(|| NCBError::config("Cannot get TTSClientData"))
            .unwrap();

        // Synthesize with retry logic
        let synthesis_result = match config.tts_type.unwrap_or(TTSType::GCP) {
            TTSType::GCP => {
                let sanitized_text = validation::sanitize_ssml(&text);
                retry_with_backoff(
                    || {
                        tts.synthesize_gcp(SynthesizeRequest {
                            input: SynthesisInput {
                                text: None,
                                ssml: Some(format!("<speak>{}</speak>", sanitized_text)),
                            },
                            voice: config.gcp_tts_voice.clone().unwrap_or_else(|| {
                                crate::tts::gcp_tts::structs::voice_selection_params::VoiceSelectionParams {
                                    languageCode: String::from("ja-JP"),
                                    name: String::from("ja-JP-Wavenet-B"),
                                    ssmlGender: String::from("neutral"),
                                }
                            }),
                            audioConfig: AudioConfig {
                                audioEncoding: String::from("mp3"),
                                speakingRate: DEFAULT_SPEAKING_RATE,
                                pitch: DEFAULT_PITCH,
                            },
                        })
                    },
                    3, // max attempts
                    std::time::Duration::from_millis(500),
                ).await
            }
            TTSType::VOICEVOX => {
                let processed_text = text.replace("<break time=\"200ms\"/>", "、");
                retry_with_backoff(
                    || {
                        tts.synthesize_voicevox(
                            &processed_text,
                            config.voicevox_speaker.unwrap_or(crate::errors::constants::DEFAULT_VOICEVOX_SPEAKER),
                        )
                    },
                    3, // max attempts
                    std::time::Duration::from_millis(500),
                ).await
            }
        };
        
        match synthesis_result {
            Ok(track) => vec![track],
            Err(e) => {
                error!(error = %e, "TTS synthesis failed");
                vec![] // Return empty vector on failure
            }
        }
    }
}

/// Helper function to get user name with proper error handling
async fn get_user_name(message: &Message, ctx: &Context) -> String {
    let member = message.member.clone();
    if let Some(_) = member {
        if let Some(guild_id) = message.guild_id {
            match guild_id.member(&ctx.http, message.author.id).await {
                Ok(member) => member.read_name(),
                Err(e) => {
                    warn!(
                        user_id = %message.author.id,
                        guild_id = ?message.guild_id,
                        error = %e,
                        "Failed to get guild member, using fallback name"
                    );
                    message.author.read_name()
                }
            }
        } else {
            warn!(
                guild_id = ?message.guild_id,
                "Guild not found in cache, using author name"
            );
            message.author.read_name()
        }
    } else {
        message.author.read_name()
    }
}
