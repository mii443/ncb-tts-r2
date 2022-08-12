use serenity::{prelude::Context, model::prelude::{application_command::ApplicationCommandInteraction, InteractionApplicationCommandCallbackDataFlags}};

use crate::{tts::{voicevox::voicevox::VOICEVOX, tts_type::TTSType}, data::DatabaseClientData};

pub async fn config_command(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<(), Box<dyn std::error::Error>> {
    let data_read = ctx.data.read().await;

    let config = {
        let database = data_read.get::<DatabaseClientData>().expect("Cannot get DatabaseClientData").clone();
        let mut database = database.lock().await;
        database.get_user_config_or_default(command.user.id.0).await.unwrap().unwrap()
    };

    let voicevox_speaker = config.voicevox_speaker.unwrap_or(1);
    let tts_type = config.tts_type.unwrap_or(TTSType::GCP);

    command.create_interaction_response(&ctx.http, |f| {
        f.interaction_response_data(|d| {
            d.content("読み上げ設定")
                .components(|c| {
                    c.create_action_row(|a| {
                        a.create_select_menu(|m| {
                            m.custom_id("TTS_CONFIG_ENGINE")
                                .options(|o| {
                                    o.create_option(|co| {
                                        co.label("Google TTS")
                                            .value("TTS_CONFIG_ENGINE_SELECTED_GOOGLE")
                                            .default_selection(tts_type == TTSType::GCP)
                                    }).create_option(|co| {
                                        co.label("VOICEVOX")
                                            .value("TTS_CONFIG_ENGINE_SELECTED_VOICEVOX")
                                            .default_selection(tts_type == TTSType::VOICEVOX)
                                    })
                                }).placeholder("読み上げAPIを選択")
                        })
                    }).create_action_row(|a| {
                        a.create_select_menu(|m| {
                            m.custom_id("TTS_CONFIG_VOICEVOX_SPEAKER")
                                .options(|o| {
                                    let mut o = o;
                                    for (name, value) in VOICEVOX::get_speakers() {
                                        o = o.create_option(|co| {
                                            co.label(name)
                                                .value(format!("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_{}", value))
                                                .default_selection(value == voicevox_speaker)
                                        })
                                    }
                                    o
                                }).placeholder("VOICEVOX Speakerを指定")
                        })
                    })
                }).flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
        })
    }).await?;
    Ok(())
}
