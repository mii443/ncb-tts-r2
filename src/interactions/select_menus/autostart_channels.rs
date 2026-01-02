//! Autostart channel select menu handlers

use crate::{errors::Result, interactions::utils};
use serenity::{all::ComponentInteraction, prelude::Context};

/// Handle "SET_AUTOSTART_CHANNEL" select
/// Sets or clears the autostart voice channel
pub async fn handle_voice_channel_select(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let channel_id = utils::parse_select_value(interaction, "SET_AUTOSTART_CHANNEL_")?;
    let guild_id = utils::extract_guild_id(interaction)?;

    let mut config = utils::get_server_config(ctx, guild_id).await?;
    config.autostart_channel_id = if channel_id == 0 {
        None
    } else {
        Some(channel_id)
    };
    utils::set_server_config(ctx, guild_id, config).await?;

    let response_content = if channel_id != 0 {
        "自動参加チャンネルを設定しました。"
    } else {
        "自動参加チャンネルを解除しました。"
    };

    utils::update_interaction_message(ctx, interaction, response_content).await?;

    Ok(())
}

/// Handle "SET_AUTOSTART_TEXT_CHANNEL" select
/// Sets or clears the autostart text channel
pub async fn handle_text_channel_select(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let channel_id =
        utils::parse_select_value(interaction, "SET_AUTOSTART_TEXT_CHANNEL_")?;
    let guild_id = utils::extract_guild_id(interaction)?;

    let mut config = utils::get_server_config(ctx, guild_id).await?;
    config.autostart_text_channel_id = if channel_id == 0 {
        None
    } else {
        Some(channel_id)
    };
    utils::set_server_config(ctx, guild_id, config).await?;

    let response_content = if channel_id != 0 {
        "自動参加テキストチャンネルを設定しました。"
    } else {
        "自動参加テキストチャンネルを解除しました。"
    };

    utils::update_interaction_message(ctx, interaction, response_content).await?;

    Ok(())
}
