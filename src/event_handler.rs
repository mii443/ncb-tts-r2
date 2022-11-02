use crate::{
    commands::{config::config_command, setup::setup_command, stop::stop_command},
    data::DatabaseClientData,
    events,
    tts::tts_type::TTSType,
};
use serenity::{
    async_trait,
    client::{Context, EventHandler},
    model::{
        channel::Message,
        gateway::Ready,
        prelude::interaction::{Interaction, MessageFlags},
        voice::VoiceState,
    },
};

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, message: Message) {
        events::message_receive::message(ctx, message).await
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        events::ready::ready(ctx, ready).await
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction.clone() {
            let name = &*command.data.name;
            match name {
                "setup" => setup_command(&ctx, &command).await.unwrap(),
                "stop" => stop_command(&ctx, &command).await.unwrap(),
                "config" => config_command(&ctx, &command).await.unwrap(),
                _ => {}
            }
        }
        if let Some(message_component) = interaction.message_component() {
            if let Some(v) = message_component.data.values.get(0) {
                let data_read = ctx.data.read().await;

                let mut config = {
                    let database = data_read
                        .get::<DatabaseClientData>()
                        .expect("Cannot get DatabaseClientData")
                        .clone();
                    let mut database = database.lock().await;
                    database
                        .get_user_config_or_default(message_component.user.id.0)
                        .await
                        .unwrap()
                        .unwrap()
                };

                let res = (*v).clone();
                let mut config_changed = false;
                let mut voicevox_changed = false;
                match &*res {
                    "TTS_CONFIG_ENGINE_SELECTED_GOOGLE" => {
                        config.tts_type = Some(TTSType::GCP);
                        config_changed = true;
                    }
                    "TTS_CONFIG_ENGINE_SELECTED_VOICEVOX" => {
                        config.tts_type = Some(TTSType::VOICEVOX);
                        config_changed = true;
                    }
                    _ => {
                        if res.starts_with("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_") {
                            config.voicevox_speaker = Some(
                                i64::from_str_radix(
                                    &res.replace("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_", ""),
                                    10,
                                )
                                .unwrap(),
                            );
                            config_changed = true;
                            voicevox_changed = true;
                        }
                    }
                }

                if config_changed {
                    let database = data_read
                        .get::<DatabaseClientData>()
                        .expect("Cannot get DatabaseClientData")
                        .clone();
                    let mut database = database.lock().await;
                    database
                        .set_user_config(message_component.user.id.0, config.clone())
                        .await
                        .unwrap();

                    if voicevox_changed && config.tts_type.unwrap_or(TTSType::GCP) == TTSType::GCP {
                        message_component.create_interaction_response(&ctx.http, |f| {
                            f.interaction_response_data(|d| {
                                d.content("設定しました\nこの音声を使うにはAPIをGoogleからVOICEVOXに変更する必要があります。")
                                    .flags(MessageFlags::EPHEMERAL)
                            })
                        }).await.unwrap();
                    } else {
                        message_component
                            .create_interaction_response(&ctx.http, |f| {
                                f.interaction_response_data(|d| {
                                    d.content("設定しました").flags(MessageFlags::EPHEMERAL)
                                })
                            })
                            .await
                            .unwrap();
                    }
                }
            }
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        events::voice_state_update::voice_state_update(ctx, old, new).await
    }
}
