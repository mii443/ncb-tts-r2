use crate::{
    commands::{
        config::config_command, setup::setup_command, skip::skip_command, stop::stop_command,
    },
    data::DatabaseClientData,
    database::dictionary::Rule,
    errors::{constants::*, validation},
    events,
    tts::tts_type::TTSType,
};
use serenity::{
    all::{
        ActionRowComponent, ButtonStyle, ComponentInteractionDataKind, CreateActionRow,
        CreateButton, CreateEmbed, CreateInputText, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateModal, CreateSelectMenu, CreateSelectMenuKind,
        CreateSelectMenuOption, InputTextStyle,
    },
    async_trait,
    client::{Context, EventHandler},
    model::{
        application::Interaction, channel::Message, gateway::Ready, prelude::ChannelType,
        voice::VoiceState,
    },
};

#[derive(Clone, Debug)]
pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    #[tracing::instrument]
    async fn message(&self, ctx: Context, message: Message) {
        events::message_receive::message(ctx, message).await
    }

    #[tracing::instrument]
    async fn ready(&self, ctx: Context, ready: Ready) {
        events::ready::ready(ctx, ready).await
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction.clone() {
            let name = &*command.data.name;
            match name {
                "setup" => setup_command(&ctx, &command).await.unwrap(),
                "stop" => stop_command(&ctx, &command).await.unwrap(),
                "config" => config_command(&ctx, &command).await.unwrap(),
                "skip" => skip_command(&ctx, &command).await.unwrap(),
                _ => {}
            }
        }
        if let Interaction::Modal(modal) = interaction.clone() {
            if modal.data.custom_id != TTS_CONFIG_SERVER_ADD_DICTIONARY {
                return;
            }

            let rows = modal.data.components.clone();
            
            // Extract rule name with proper error handling
            let rule_name = match rows.get(0)
                .and_then(|row| row.components.get(0))
                .and_then(|component| {
                    if let ActionRowComponent::InputText(text) = component {
                        text.value.as_ref()
                    } else {
                        None
                    }
                }) {
                Some(name) => {
                    if let Err(e) = validation::validate_rule_name(name) {
                        tracing::error!("Invalid rule name: {}", e);
                        return;
                    }
                    name.clone()
                },
                None => {
                    tracing::error!("Cannot extract rule name from modal");
                    return;
                }
            };

            // Extract 'from' field with validation
            let from = match rows.get(1)
                .and_then(|row| row.components.get(0))
                .and_then(|component| {
                    if let ActionRowComponent::InputText(text) = component {
                        text.value.as_ref()
                    } else {
                        None
                    }
                }) {
                Some(pattern) => {
                    if let Err(e) = validation::validate_regex_pattern(pattern) {
                        tracing::error!("Invalid regex pattern: {}", e);
                        return;
                    }
                    pattern.clone()
                },
                None => {
                    tracing::error!("Cannot extract regex pattern from modal");
                    return;
                }
            };

            // Extract 'to' field with validation
            let to = match rows.get(2)
                .and_then(|row| row.components.get(0))
                .and_then(|component| {
                    if let ActionRowComponent::InputText(text) = component {
                        text.value.as_ref()
                    } else {
                        None
                    }
                }) {
                Some(replacement) => {
                    if let Err(e) = validation::validate_replacement_text(replacement) {
                        tracing::error!("Invalid replacement text: {}", e);
                        return;
                    }
                    replacement.clone()
                },
                None => {
                    tracing::error!("Cannot extract replacement text from modal");
                    return;
                }
            };

            let rule = Rule {
                id: rule_name.clone(),
                is_regex: true,
                rule: from.clone(),
                to: to.clone(),
            };

            let data_read = ctx.data.read().await;

            let mut config = {
                let database = match data_read.get::<DatabaseClientData>() {
                    Some(db) => db.clone(),
                    None => {
                        tracing::error!("Cannot get DatabaseClientData");
                        return;
                    }
                };

                match database.get_server_config_or_default(modal.guild_id.unwrap().get()).await {
                    Ok(Some(config)) => config,
                    Ok(None) => {
                        tracing::error!("No server config found");
                        return;
                    },
                    Err(e) => {
                        tracing::error!("Database error: {}", e);
                        return;
                    }
                }
            };
            config.dictionary.rules.push(rule);

            {
                let database = match data_read.get::<DatabaseClientData>() {
                    Some(db) => db.clone(),
                    None => {
                        tracing::error!("Cannot get DatabaseClientData");
                        return;
                    }
                };

                if let Err(e) = database.set_server_config(modal.guild_id.unwrap().get(), config).await {
                    tracing::error!("Failed to save server config: {}", e);
                    return;
                }
                modal
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new().content(format!(
                                "辞書を追加しました\n名前: {}\n変換元: {}\n変換後: {}",
                                rule_name, from, to
                            )),
                        ),
                    )
                    .await
                    .unwrap();
            }
        }
        if let Some(message_component) = interaction.message_component() {
            match &*message_component.data.custom_id {
                id if id == TTS_CONFIG_SERVER_SET_VOICE_STATE_ANNOUNCE => {
                    let data_read = ctx.data.read().await;
                    let mut config = {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().get())
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    config.voice_state_announce =
                        Some(!config.voice_state_announce.unwrap_or(true));
                    let state = config.voice_state_announce.unwrap_or(true);

                    {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .set_server_config(message_component.guild_id.unwrap().get(), config)
                            .await
                            .unwrap();
                    }

                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new().content(format!(
                                    "入退出アナウンス通知を{}へ切り替えました。",
                                    if state { "`有効`" } else { "`無効`" }
                                )),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_SET_READ_USERNAME => {
                    let data_read = ctx.data.read().await;
                    let mut config = {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().get())
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    config.read_username = Some(!config.read_username.unwrap_or(true));
                    let state = config.read_username.unwrap_or(true);

                    {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .set_server_config(message_component.guild_id.unwrap().get(), config)
                            .await
                            .unwrap();
                    }

                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new().content(format!(
                                    "ユーザー名読み上げを{}へ切り替えました。",
                                    if state { "`有効`" } else { "`無効`" }
                                )),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU => {
                    let i = usize::from_str_radix(
                        &match message_component.data.kind {
                            ComponentInteractionDataKind::StringSelect { ref values, .. } => {
                                values[0].clone()
                            }
                            _ => panic!("Cannot get index"),
                        },
                        10,
                    )
                    .unwrap();
                    let data_read = ctx.data.read().await;

                    let mut config = {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().get())
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

                        database
                            .set_server_config(message_component.guild_id.unwrap().get(), config)
                            .await
                            .unwrap();
                    }

                    message_component
                        .create_response(
                            &ctx,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content("辞書を削除しました"),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON => {
                    let data_read = ctx.data.read().await;

                    let config = {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().get())
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content("削除する辞書内容を選択してください")
                                    .components(vec![CreateActionRow::SelectMenu(
                                        CreateSelectMenu::new(
                                            "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU",
                                            CreateSelectMenuKind::String {
                                                options: {
                                                    let mut options = vec![];
                                                    for (i, rule) in
                                                        config.dictionary.rules.iter().enumerate()
                                                    {
                                                        let option = CreateSelectMenuOption::new(
                                                            rule.id.clone(),
                                                            i.to_string(),
                                                        )
                                                        .description(format!(
                                                            "{} -> {}",
                                                            rule.rule.clone(),
                                                            rule.to.clone()
                                                        ));
                                                        options.push(option);
                                                    }
                                                    options
                                                },
                                            },
                                        )
                                        .max_values(1)
                                        .min_values(0),
                                    )]),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON => {
                    let config = {
                        let data_read = ctx.data.read().await;
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().get())
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new().content("").embed(
                                    CreateEmbed::new().title("辞書一覧").fields({
                                        let mut fields = vec![];
                                        for rule in config.dictionary.rules {
                                            let field = (
                                                rule.id.clone(),
                                                format!("{} -> {}", rule.rule, rule.to),
                                                true,
                                            );
                                            fields.push(field);
                                        }
                                        fields
                                    }),
                                ),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON => {
                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Modal(
                                CreateModal::new("TTS_CONFIG_SERVER_ADD_DICTIONARY", "辞書追加")
                                    .components({
                                        vec![
                                            CreateActionRow::InputText(
                                                CreateInputText::new(
                                                    InputTextStyle::Short,
                                                    "rule_name",
                                                    "辞書名",
                                                )
                                                .required(true),
                                            ),
                                            CreateActionRow::InputText(
                                                CreateInputText::new(
                                                    InputTextStyle::Paragraph,
                                                    "from",
                                                    "変換元（正規表現）",
                                                )
                                                .required(true),
                                            ),
                                            CreateActionRow::InputText(
                                                CreateInputText::new(
                                                    InputTextStyle::Short,
                                                    "to",
                                                    "変換先",
                                                )
                                                .required(true),
                                            ),
                                        ]
                                    }),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == SET_AUTOSTART_CHANNEL => {
                    let autostart_channel_id = match message_component.data.kind {
                        ComponentInteractionDataKind::StringSelect { ref values, .. } => {
                            if values.len() == 0 {
                                None
                            } else if values[0] == "SET_AUTOSTART_CHANNEL_CLEAR" {
                                None
                            } else {
                                Some(
                                    u64::from_str_radix(
                                        &values[0].strip_prefix("SET_AUTOSTART_CHANNEL_").unwrap(),
                                        10,
                                    )
                                    .unwrap(),
                                )
                            }
                        }
                        _ => panic!("Cannot get index"),
                    };
                    {
                        let data_read = ctx.data.read().await;
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        let mut config = database
                            .get_server_config_or_default(message_component.guild_id.unwrap().get())
                            .await
                            .unwrap()
                            .unwrap();
                        config.autostart_channel_id = autostart_channel_id;
                        database
                            .set_server_config(message_component.guild_id.unwrap().get(), config)
                            .await
                            .unwrap();
                    };

                    let response_content = if autostart_channel_id.is_some() {
                        "自動参加チャンネルを設定しました。"
                    } else {
                        "自動参加チャンネルを解除しました。"
                    };

                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content(response_content),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL => {
                    let config = {
                        let data_read = ctx.data.read().await;
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .get_server_config_or_default(message_component.guild_id.unwrap().get())
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    let autostart_channel_id = config.autostart_channel_id.unwrap_or(0);

                    let channels = message_component
                        .guild_id
                        .unwrap()
                        .channels(&ctx.http)
                        .await
                        .unwrap();

                    let mut options = Vec::new();
                    
                    // 解除オプションを追加
                    let clear_option = CreateSelectMenuOption::new(
                        "解除",
                        "SET_AUTOSTART_CHANNEL_CLEAR",
                    )
                    .description("自動参加チャンネルを解除します")
                    .default_selection(autostart_channel_id == 0);
                    options.push(clear_option);

                    for (id, channel) in channels {
                        if channel.kind != ChannelType::Voice {
                            continue;
                        }

                        let description = channel
                            .topic
                            .unwrap_or_else(|| String::from("No topic provided."));
                        let option = CreateSelectMenuOption::new(
                            &channel.name,
                            format!("SET_AUTOSTART_CHANNEL_{}", id.get()),
                        )
                        .description(description)
                        .default_selection(channel.id.get() == autostart_channel_id);

                        options.push(option);
                    }

                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content("自動参加チャンネル設定")
                                    .components(vec![
                                        CreateActionRow::SelectMenu(
                                            CreateSelectMenu::new(
                                                "SET_AUTOSTART_CHANNEL",
                                                CreateSelectMenuKind::String { options },
                                            )
                                            .min_values(0)
                                            .max_values(1),
                                        ),
                                        CreateActionRow::Buttons(vec![CreateButton::new(
                                            "TTS_CONFIG_SERVER_BACK",
                                        )
                                        .label("← サーバー設定に戻る")
                                        .style(ButtonStyle::Secondary)]),
                                    ]),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_BACK => {
                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content("サーバー設定")
                                    .components(vec![CreateActionRow::Buttons(vec![
                                        CreateButton::new("TTS_CONFIG_SERVER_DICTIONARY")
                                            .label("辞書管理")
                                            .style(ButtonStyle::Primary),
                                        CreateButton::new(
                                            "TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL",
                                        )
                                        .label("自動参加チャンネル")
                                        .style(ButtonStyle::Primary),
                                        CreateButton::new(
                                            "TTS_CONFIG_SERVER_SET_VOICE_STATE_ANNOUNCE",
                                        )
                                        .label("入退出アナウンス通知切り替え")
                                        .style(ButtonStyle::Primary),
                                        CreateButton::new("TTS_CONFIG_SERVER_SET_READ_USERNAME")
                                            .label("ユーザー名読み上げ切り替え")
                                            .style(ButtonStyle::Primary),
                                    ])]),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER => {
                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content("サーバー設定")
                                    .components(vec![CreateActionRow::Buttons(vec![
                                        CreateButton::new("TTS_CONFIG_SERVER_DICTIONARY")
                                            .label("辞書管理")
                                            .style(ButtonStyle::Primary),
                                        CreateButton::new(
                                            "TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL",
                                        )
                                        .label("自動参加チャンネル")
                                        .style(ButtonStyle::Primary),
                                        CreateButton::new(
                                            "TTS_CONFIG_SERVER_SET_VOICE_STATE_ANNOUNCE",
                                        )
                                        .label("入退出アナウンス通知切り替え")
                                        .style(ButtonStyle::Primary),
                                        CreateButton::new("TTS_CONFIG_SERVER_SET_READ_USERNAME")
                                            .label("ユーザー名読み上げ切り替え")
                                            .style(ButtonStyle::Primary),
                                    ])]),
                            ),
                        )
                        .await
                        .unwrap();
                }
                id if id == TTS_CONFIG_SERVER_DICTIONARY => {
                    message_component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content("辞書管理")
                                    .components(vec![
                                        CreateActionRow::Buttons(vec![
                                            CreateButton::new(
                                                "TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON",
                                            )
                                            .label("辞書を追加")
                                            .style(ButtonStyle::Primary),
                                            CreateButton::new(
                                                "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON",
                                            )
                                            .label("辞書を削除")
                                            .style(ButtonStyle::Danger),
                                            CreateButton::new(
                                                "TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON",
                                            )
                                            .label("辞書一覧")
                                            .style(ButtonStyle::Primary),
                                        ]),
                                        CreateActionRow::Buttons(vec![CreateButton::new(
                                            "TTS_CONFIG_SERVER_BACK",
                                        )
                                        .label("← サーバー設定に戻る")
                                        .style(ButtonStyle::Secondary)]),
                                    ]),
                            ),
                        )
                        .await
                        .unwrap();
                }
                _ => {}
            }
            match message_component.data.kind {
                ComponentInteractionDataKind::StringSelect { ref values, .. }
                    if !values.is_empty() =>
                {
                    let res = &values[0].clone();
                    let data_read = ctx.data.read().await;

                    let mut config = {
                        let database = data_read
                            .get::<DatabaseClientData>()
                            .expect("Cannot get DatabaseClientData")
                            .clone();

                        database
                            .get_user_config_or_default(message_component.user.id.get())
                            .await
                            .unwrap()
                            .unwrap()
                    };

                    let mut config_changed = false;
                    let mut voicevox_changed = false;

                    match res.as_str() {
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
                                let speaker_id = res
                                    .strip_prefix("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_")
                                    .and_then(|id_str| id_str.parse::<i64>().ok())
                                    .expect("Invalid speaker ID format");

                                config.voicevox_speaker = Some(speaker_id);
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

                        database
                            .set_user_config(message_component.user.id.get(), config.clone())
                            .await
                            .unwrap();

                        let response_content = if voicevox_changed
                            && config.tts_type.unwrap_or(TTSType::GCP) == TTSType::GCP
                        {
                            "設定しました\nこの音声を使うにはAPIをGoogleからVOICEVOXに変更する必要があります。"
                        } else {
                            "設定しました"
                        };

                        message_component
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(response_content)
                                        .ephemeral(true),
                                ),
                            )
                            .await
                            .unwrap();
                    }
                }
                _ => {}
            }
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        events::voice_state_update::voice_state_update(ctx, old, new).await
    }
}
