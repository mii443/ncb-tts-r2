use async_trait::async_trait;
use regex::Regex;
use serenity::{model::prelude::Message, prelude::Context};
use songbird::input::cached::Compressed;

use crate::{
    data::{DatabaseClientData, TTSClientData}, implement::member_name::ReadName, tts::{
        gcp_tts::structs::{
            audio_config::AudioConfig, synthesis_input::SynthesisInput,
            synthesize_request::SynthesizeRequest,
        },
        instance::TTSInstance,
        message::TTSMessage,
        tts_type::TTSType,
    }
};

#[async_trait]
impl TTSMessage for Message {
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> String {
        let data_read = ctx.data.read().await;

        let config = {
            let database = data_read
                .get::<DatabaseClientData>()
                .expect("Cannot get DatabaseClientData")
                .clone();
            let mut database = database.lock().await;
            database
                .get_server_config_or_default(instance.guild.get())
                .await
                .unwrap()
                .unwrap()
        };
        let mut text = self.content.clone();
        for rule in config.dictionary.rules {
            if rule.is_regex {
                let regex = Regex::new(&rule.rule).unwrap();
                text = regex.replace_all(&text, rule.to).to_string();
            } else {
                text = text.replace(&rule.rule, &rule.to);
            }
        }
        let mut res = if let Some(before_message) = &instance.before_message {
            if before_message.author.id == self.author.id {
                text.clone()
            } else {
                let member = self.member.clone();
                let name = if let Some(_) = member {
                    let guild = ctx.cache.guild(self.guild_id.unwrap()).unwrap().clone();
                    guild.member(&ctx.http, self.author.id).await.unwrap().read_name()
                } else {
                    self.author.read_name()
                };
                format!("{}さんの発言<break time=\"200ms\"/>{}", name, text)
            }
        } else {
            let member = self.member.clone();
            let name = if let Some(_) = member {
                let guild = ctx.cache.guild(self.guild_id.unwrap()).unwrap().clone();
                guild.member(&ctx.http, self.author.id).await.unwrap().read_name()
            } else {
                self.author.read_name()
            };
            format!("{}さんの発言<break time=\"200ms\"/>{}", name, text)
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

    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> Compressed {
        let text = self.parse(instance, ctx).await;

        let data_read = ctx.data.read().await;
        let storage = data_read
            .get::<TTSClientData>()
            .expect("Cannot get GCP TTSClientStorage")
            .clone();
        let mut tts = storage.lock().await;

        let config = {
            let database = data_read
                .get::<DatabaseClientData>()
                .expect("Cannot get DatabaseClientData")
                .clone();
            let mut database = database.lock().await;
            database
                .get_user_config_or_default(self.author.id.get())
                .await
                .unwrap()
                .unwrap()
        };

        let audio = match config.tts_type.unwrap_or(TTSType::GCP) {
            TTSType::GCP => tts
                .synthesize_gcp(SynthesizeRequest {
                    input: SynthesisInput {
                        text: None,
                        ssml: Some(format!("<speak>{}</speak>", text)),
                    },
                    voice: config.gcp_tts_voice.unwrap(),
                    audioConfig: AudioConfig {
                        audioEncoding: String::from("mp3"),
                        speakingRate: 1.2f32,
                        pitch: 1.0f32,
                    },
                })
                .await
                .unwrap(),

            TTSType::VOICEVOX => tts
                .synthesize_voicevox(
                    &text.replace("<break time=\"200ms\"/>", "、"),
                    config.voicevox_speaker.unwrap_or(1),
                )
                .await
                .unwrap(),
        };

        audio
    }
}
