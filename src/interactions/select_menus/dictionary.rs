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

    let config = utils::get_server_config(ctx, guild_id).await?;
    let selected = config
        .dictionary
        .rules
        .get(index)
        .ok_or_else(|| {
            crate::errors::NCBError::invalid_input(
                "辞書が変更されています。メニューを開き直してください。",
            )
        })?
        .clone();
    utils::update_server_config(ctx, guild_id, |config| {
        let position = config
            .dictionary
            .rules
            .iter()
            .position(|rule| rule == &selected)
            .ok_or_else(|| {
                crate::errors::NCBError::invalid_input("選択した辞書は既に変更されています。")
            })?;
        config.dictionary.rules.remove(position);
        Ok(())
    })
    .await?;

    utils::update_interaction_message(ctx, interaction, "辞書を削除しました").await?;

    Ok(())
}
