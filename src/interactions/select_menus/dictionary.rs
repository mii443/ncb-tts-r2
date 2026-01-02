//! Dictionary removal select menu handler

use crate::{errors::Result, interactions::utils};
use serenity::{all::ComponentInteraction, prelude::Context};

/// Handle "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU" select
/// Removes the selected dictionary entry
pub async fn handle_remove_dictionary_select(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let index = utils::parse_select_index(interaction)?;
    let guild_id = utils::extract_guild_id(interaction)?;

    let mut config = utils::get_server_config(ctx, guild_id).await?;
    config.dictionary.rules.remove(index);
    utils::set_server_config(ctx, guild_id, config).await?;

    utils::update_interaction_message(ctx, interaction, "辞書を削除しました").await?;

    Ok(())
}
