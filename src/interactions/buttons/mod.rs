//! Button interaction handlers

mod autostart_channel;
mod config_toggles;
mod dictionary_management;
mod navigation;
mod voicevox_config;

use crate::errors::Result;
use serenity::{all::ComponentInteraction, prelude::Context};

/// Handle button interactions
pub async fn handle_button(ctx: &Context, interaction: &ComponentInteraction) -> Result<()> {
    use crate::errors::constants::*;

    match interaction.data.custom_id.as_str() {
        // Config toggles
        TTS_CONFIG_SERVER_SET_VOICE_STATE_ANNOUNCE => {
            config_toggles::handle_voice_state_announce_toggle(ctx, interaction).await
        }
        TTS_CONFIG_SERVER_SET_READ_USERNAME => {
            config_toggles::handle_read_username_toggle(ctx, interaction).await
        }

        // Dictionary management
        TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON => {
            dictionary_management::handle_show_add_modal(ctx, interaction).await
        }
        TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON => {
            dictionary_management::handle_show_remove_menu(ctx, interaction).await
        }
        TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON => {
            dictionary_management::handle_show_dictionary_list(ctx, interaction).await
        }

        // Navigation
        TTS_CONFIG_SERVER => navigation::handle_show_server_config(ctx, interaction).await,
        TTS_CONFIG_SERVER_BACK => {
            navigation::handle_back_to_server_config(ctx, interaction).await
        }
        TTS_CONFIG_SERVER_DICTIONARY => {
            navigation::handle_show_dictionary_menu(ctx, interaction).await
        }

        // Autostart channel
        TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL => {
            autostart_channel::handle_show_autostart_menu(ctx, interaction).await
        }

        // VOICEVOX configuration
        TTS_CONFIG_VOICEVOX => voicevox_config::handle_show_voicevox_config(ctx, interaction).await,
        TTS_CONFIG_BACK_TO_MAIN => navigation::handle_back_to_main_config(ctx, interaction).await,

        // VOICEVOX page navigation (dynamic routing)
        custom_id if custom_id.starts_with("TTS_CONFIG_VOICEVOX_PAGE_") => {
            // Parse page number from custom_id
            if let Some(page_str) = custom_id.strip_prefix("TTS_CONFIG_VOICEVOX_PAGE_") {
                if let Ok(page) = page_str.parse::<usize>() {
                    voicevox_config::handle_voicevox_page(ctx, interaction, page).await
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            }
        }

        _ => Ok(()),
    }
}
