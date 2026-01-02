//! Select menu interaction handlers

mod autostart_channels;
mod dictionary;
mod tts_config;

use crate::errors::Result;
use serenity::{all::{ComponentInteraction, ComponentInteractionDataKind}, prelude::Context};

/// Handle select menu interactions
pub async fn handle_select_menu(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    use crate::errors::constants::*;

    match interaction.data.custom_id.as_str() {
        TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU => {
            dictionary::handle_remove_dictionary_select(ctx, interaction).await
        }
        SET_AUTOSTART_CHANNEL => {
            autostart_channels::handle_voice_channel_select(ctx, interaction).await
        }
        SET_AUTOSTART_TEXT_CHANNEL => {
            autostart_channels::handle_text_channel_select(ctx, interaction).await
        }
        _ => {
            // Handle dynamic TTS config selects
            if let ComponentInteractionDataKind::StringSelect { ref values, .. } =
                interaction.data.kind
            {
                if !values.is_empty() {
                    tts_config::handle_tts_config_select(ctx, interaction).await
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            }
        }
    }
}
