//! Navigation button handlers
//!
//! Handles buttons for navigating between config menus

use crate::{
    data::DatabaseClientData,
    errors::{constants::*, NCBError, Result},
    interactions::utils,
    tts::tts_type::TTSType,
};
use serenity::{
    all::{
        ButtonStyle, ComponentInteraction, CreateActionRow, CreateButton,
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateSelectMenu,
        CreateSelectMenuKind, CreateSelectMenuOption,
    },
    prelude::Context,
};

/// Handle "TTS_CONFIG_SERVER" button - show main server config menu
pub async fn handle_show_server_config(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("サーバー設定")
                    .components(utils::build_server_config_buttons()),
            ),
        )
        .await
        .map_err(|e| crate::errors::NCBError::Discord(e))?;
    Ok(())
}

/// Handle "TTS_CONFIG_SERVER_BACK" button - return to server config menu
pub async fn handle_back_to_server_config(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    // Same as show_server_config
    handle_show_server_config(ctx, interaction).await
}

/// Handle "TTS_CONFIG_SERVER_DICTIONARY" button - show dictionary management menu
pub async fn handle_show_dictionary_menu(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("辞書管理")
                    .components(utils::build_dictionary_menu_buttons()),
            ),
        )
        .await
        .map_err(|e| crate::errors::NCBError::Discord(e))?;
    Ok(())
}

/// Handle "TTS_CONFIG_BACK_TO_MAIN" button - return to main user config menu
pub async fn handle_back_to_main_config(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let data_read = ctx.data.read().await;

    // Get user config
    let config = {
        let database = data_read
            .get::<DatabaseClientData>()
            .ok_or_else(|| NCBError::config("Cannot get DatabaseClientData"))?
            .clone();
        database
            .get_user_config_or_default(interaction.user.id.get())
            .await
            .map_err(|e| NCBError::database(format!("Failed to get user config: {}", e)))?
            .ok_or_else(|| NCBError::config("User config not found"))?
    };

    let tts_type = config.tts_type.unwrap_or(TTSType::GCP);

    // Build main config components
    let engine_select = CreateActionRow::SelectMenu(
        CreateSelectMenu::new(
            "TTS_CONFIG_ENGINE",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new(
                        "Google TTS",
                        TTS_CONFIG_ENGINE_SELECTED_GOOGLE,
                    )
                    .default_selection(tts_type == TTSType::GCP),
                    CreateSelectMenuOption::new(
                        "VOICEVOX",
                        TTS_CONFIG_ENGINE_SELECTED_VOICEVOX,
                    )
                    .default_selection(tts_type == TTSType::VOICEVOX),
                ],
            },
        )
        .placeholder("読み上げAPIを選択"),
    );

    let voicevox_button = CreateActionRow::Buttons(vec![CreateButton::new(TTS_CONFIG_VOICEVOX)
        .label("VOICEVOX設定")
        .style(ButtonStyle::Primary)]);

    let server_button = CreateActionRow::Buttons(vec![CreateButton::new(TTS_CONFIG_SERVER)
        .label("サーバー設定")
        .style(ButtonStyle::Primary)]);

    let components = vec![engine_select, voicevox_button, server_button];

    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("読み上げ設定")
                    .components(components),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;

    Ok(())
}
