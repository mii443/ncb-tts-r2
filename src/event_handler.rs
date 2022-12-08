use crate::{
    commands::{
        config::config_command, setup::setup_command, skip::skip_command, stop::stop_command,
    },
    data::DatabaseClientData,
    database::dictionary::Rule,
    events,
    tts::tts_type::TTSType,
};
use serenity::{
    async_trait,
    client::{Context, EventHandler},
    model::{
        channel::Message,
        gateway::Ready,
        prelude::{
            component::{ActionRowComponent, ButtonStyle, InputTextStyle},
            interaction::{Interaction, InteractionResponseType, MessageFlags},
        },
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
                "skip" => skip_command(&ctx, &command).await.unwrap(),
                _ => {}
            }
        }
        if let Interaction::ModalSubmit(modal) = interaction.clone() {
            if modal.data.custom_id != "TTS_CONFIG_SERVER_ADD_DICTIONARY" {
                return;
            }

            let rows = modal.data.components.clone();
            let rule_name =
                if let ActionRowComponent::InputText(text) = rows[0].components[0].clone() {
                    text.value
                } else {
                    panic!("Cannot get rule name");
                };

            let from = if let ActionRowComponent::InputText(text) = rows[1].components[0].clone() {
                text.value
            } else {
                panic!("Cannot get from");
            };

            let to = if let ActionRowComponent::InputText(text) = rows[2].components[0].clone() {
                text.value
            } else {
                panic!("Cannot get to");
            };

            let rule = Rule {
                id: rule_name.clone(),
                is_regex: true,
                rule: from.clone(),
                to: to.clone(),
            };

            let data_read = ctx.data.read().await;

            let mut config = {
                let database = data_read
                    .get::<DatabaseClientData>()
                    .expect("Cannot get DatabaseClientData")
                    .clone();
                let mut database = database.lock().await;
                database
                    .get_server_config_or_default(modal.guild_id.unwrap().0)
                    .await
                    .unwrap()
                    .unwrap()
            };
            config.dictionary.rules.push(rule);

            {
                let database = data_read
                    .get::<DatabaseClientData>()
                    .expect("Cannot get DatabaseClientData")
                    .clone();
                let mut database = database.lock().await;
                database
                    .set_server_config(modal.guild_id.unwrap().0, config)
                    .await
                    .unwrap();
                modal
                    .create_interaction_response(&ctx.http, |f| {
                        f.kind(InteractionResponseType::UpdateMessage)
                            .interaction_response_data(|d| {
                                d.custom_id("TTS_CONFIG_SERVER_ADD_DICTIONARY_RESPONSE")
                                    .content(format!(
                                        "辞書を追加しました\n名前: {}\n変換元: {}\n変換後: {}",
                                        rule_name, from, to
                                    ))
                            })
                    })
                    .await
                    .unwrap();
            }
        }
        if let Some(message_component) = interaction.message_component() {
            match &*message_component.data.custom_id {
                "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU" => {
                    let i = usize::from_str_radix(&message_component.data.values[0], 10).unwrap();
                    let data_read = ctx.data.read().await;

                    let mut config = {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();
                        let mut database = database.lock().await;
                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().0)
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    config.dictionary.rules.remove(i);
                    {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();
                        let mut database = database.lock().await;
                        database
                            .set_server_config(message_component.guild_id.unwrap().0, config)
                            .await
                            .unwrap();
                    }

                    message_component
                        .create_interaction_response(&ctx, |f| {
                            f.kind(InteractionResponseType::UpdateMessage)
                                .interaction_response_data(|d| {
                                    d.custom_id("DICTIONARY_REMOVED")
                                        .content("辞書を削除しました")
                                        .components(|c| c)
                                })
                        })
                        .await
                        .unwrap();
                }
                "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON" => {
                    let data_read = ctx.data.read().await;

                    let config = {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();
                        let mut database = database.lock().await;
                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().0)
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    message_component
                        .create_interaction_response(&ctx.http, |f| {
                            f.kind(InteractionResponseType::UpdateMessage)
                                .interaction_response_data(|d| {
                                    d.custom_id("TTS_CONFIG_SERVER_REMOVE_DICTIONARY")
                                        .content("削除する辞書内容を選択してください")
                                        .components(|c| {
                                            c.create_action_row(|a| {
                                                a.create_select_menu(|s| {
                                                    s.custom_id(
                                                        "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU",
                                                    )
                                                    .options(|o| {
                                                        let mut o = o;
                                                        for (i, rule) in config
                                                            .dictionary
                                                            .rules
                                                            .iter()
                                                            .enumerate()
                                                        {
                                                            o = o.create_option(|c| {
                                                                c.label(rule.id.clone())
                                                                    .value(i)
                                                                    .description(format!(
                                                                        "{} -> {}",
                                                                        rule.rule.clone(),
                                                                        rule.to.clone()
                                                                    ))
                                                            });
                                                        }
                                                        o
                                                    })
                                                    .max_values(1)
                                                    .min_values(0)
                                                })
                                            })
                                        })
                                })
                        })
                        .await
                        .unwrap();
                }
                "TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON" => {
                    let config = {
                        let data_read = ctx.data.read().await;
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();
                        let mut database = database.lock().await;
                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().0)
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    message_component
                        .create_interaction_response(&ctx.http, |f| {
                            f.kind(InteractionResponseType::UpdateMessage)
                                .interaction_response_data(|d| {
                                    d.custom_id("DICTIONARY_LIST").content("").embed(|e| {
                                        e.title("辞書一覧");
                                        for rule in config.dictionary.rules {
                                            e.field(
                                                rule.id,
                                                format!("{} -> {}", rule.rule, rule.to),
                                                true,
                                            );
                                        }
                                        e
                                    })
                                })
                        })
                        .await
                        .unwrap();
                }
                "TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON" => {
                    message_component
                        .create_interaction_response(&ctx.http, |f| {
                            f.kind(InteractionResponseType::Modal)
                                .interaction_response_data(|d| {
                                    d.custom_id("TTS_CONFIG_SERVER_ADD_DICTIONARY")
                                        .title("辞書追加")
                                        .components(|c| {
                                            c.create_action_row(|a| {
                                                a.create_input_text(|i| {
                                                    i.style(InputTextStyle::Short)
                                                        .label("Rule name")
                                                        .custom_id("rule_name")
                                                        .required(true)
                                                })
                                            })
                                            .create_action_row(|a| {
                                                a.create_input_text(|i| {
                                                    i.style(InputTextStyle::Paragraph)
                                                        .label("From")
                                                        .custom_id("from")
                                                        .required(true)
                                                })
                                            })
                                            .create_action_row(|a| {
                                                a.create_input_text(|i| {
                                                    i.style(InputTextStyle::Short)
                                                        .label("To")
                                                        .custom_id("to")
                                                        .required(true)
                                                })
                                            })
                                        })
                                })
                        })
                        .await
                        .unwrap();
                }
                "TTS_CONFIG_SERVER" => {
                    message_component
                        .create_interaction_response(&ctx.http, |f| {
                            f.kind(InteractionResponseType::UpdateMessage)
                                .interaction_response_data(|d| {
                                    d.content("サーバー設定")
                                        .custom_id("TTS_CONFIG_SERVER")
                                        .components(|c| {
                                            c.create_action_row(|a| {
                                                a.create_button(|b| {
                                                    b.custom_id(
                                                        "TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON",
                                                    )
                                                    .label("辞書を追加")
                                                    .style(ButtonStyle::Primary)
                                                })
                                                .create_button(|b| {
                                                    b.custom_id(
                                                        "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON",
                                                    )
                                                    .label("辞書を削除")
                                                    .style(ButtonStyle::Danger)
                                                })
                                                .create_button(|b| {
                                                    b.custom_id(
                                                        "TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON",
                                                    )
                                                    .label("辞書一覧")
                                                    .style(ButtonStyle::Primary)
                                                })
                                            })
                                        })
                                })
                        })
                        .await
                        .unwrap();
                }
                _ => {}
            }
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
