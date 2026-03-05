//! Autostart channel configuration button handler
//!
//! Handles the button for configuring autostart voice and text channels

use crate::{errors::{NCBError, Result}, interactions::utils};
use serenity::{
    all::{
        ButtonStyle, ChannelType, ComponentInteraction, CreateActionRow, CreateButton,
        CreateComponent, CreateInteractionResponse, CreateInteractionResponseMessage,
        CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption,
    },
    prelude::Context,
};

/// Handle "TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL" button
/// Shows select menus for voice and text channel selection
pub async fn handle_show_autostart_menu(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let guild_id = utils::extract_guild_id(interaction)?;
    let config = utils::get_server_config(ctx, guild_id).await?;

    let autostart_channel_id = config.autostart_channel_id.unwrap_or(0);

    // Fetch channels
    let channels = interaction
        .guild_id
        .ok_or_else(|| NCBError::config("Guild not found"))?
        .channels(&ctx.http)
        .await
        .map_err(|e| NCBError::Discord(e))?;

    // Build voice channel options
    let mut voice_options = vec![];
    let clear_option = CreateSelectMenuOption::new("解除", "SET_AUTOSTART_CHANNEL_CLEAR")
        .description("自動参加チャンネルを解除します")
        .default_selection(autostart_channel_id == 0);
    voice_options.push(clear_option);

    for channel in channels.clone() {
        if channel.base.kind != ChannelType::Voice {
            continue;
        }
        let description = channel
            .topic
            .map(|t| t.into_string())
            .unwrap_or_else(|| "No topic provided.".to_string());
        let option = CreateSelectMenuOption::new(
            channel.base.name.to_string(),
            format!("SET_AUTOSTART_CHANNEL_{}", channel.id.get()),
        )
        .description(description)
        .default_selection(channel.id.get() == autostart_channel_id);
        voice_options.push(option);
    }

    // Build text channel options
    let mut text_options = vec![];
    let clear_option =
        CreateSelectMenuOption::new("解除", "SET_AUTOSTART_TEXT_CHANNEL_CLEAR")
            .description("自動参加テキストチャンネルを解除します")
            .default_selection(config.autostart_text_channel_id.is_none());
    text_options.push(clear_option);

    for channel in channels {
        if channel.base.kind != ChannelType::Text {
            continue;
        }
        let description = channel
            .topic
            .map(|t| t.into_string())
            .unwrap_or_else(|| "No topic provided.".to_string());
        let option = CreateSelectMenuOption::new(
            channel.base.name.to_string(),
            format!("SET_AUTOSTART_TEXT_CHANNEL_{}", channel.id.get()),
        )
        .description(description)
        .default_selection(
            channel.id.get() == config.autostart_text_channel_id.unwrap_or(0),
        );
        text_options.push(option);
    }

    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("自動参加チャンネル設定")
                    .components(vec![
                        CreateComponent::ActionRow(CreateActionRow::SelectMenu(
                            CreateSelectMenu::new(
                                "SET_AUTOSTART_CHANNEL",
                                CreateSelectMenuKind::String {
                                    options: voice_options.into(),
                                },
                            )
                            .min_values(0)
                            .max_values(1),
                        )),
                        CreateComponent::ActionRow(CreateActionRow::SelectMenu(
                            CreateSelectMenu::new(
                                "SET_AUTOSTART_TEXT_CHANNEL",
                                CreateSelectMenuKind::String {
                                    options: text_options.into(),
                                },
                            )
                            .min_values(0)
                            .max_values(1),
                        )),
                        CreateComponent::ActionRow(CreateActionRow::Buttons(vec![CreateButton::new(
                            "TTS_CONFIG_SERVER_BACK",
                        )
                        .label("← サーバー設定に戻る")
                        .style(ButtonStyle::Secondary)].into())),
                    ]),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;
    Ok(())
}
