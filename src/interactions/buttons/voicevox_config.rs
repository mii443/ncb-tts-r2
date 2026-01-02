//! VOICEVOX configuration page with pagination

use crate::{
    data::{DatabaseClientData, TTSClientData},
    errors::{constants::*, NCBError, Result},
};
use serenity::{
    all::{
        ButtonStyle, ComponentInteraction, CreateActionRow, CreateButton,
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateSelectMenu,
        CreateSelectMenuKind, CreateSelectMenuOption,
    },
    prelude::Context,
};

const SPEAKERS_PER_PAGE: usize = 25;

/// Handle showing the VOICEVOX configuration page (first page)
pub async fn handle_show_voicevox_config(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    show_voicevox_page(ctx, interaction, 0).await
}

/// Handle VOICEVOX page navigation
pub async fn handle_voicevox_page(
    ctx: &Context,
    interaction: &ComponentInteraction,
    page: usize,
) -> Result<()> {
    show_voicevox_page(ctx, interaction, page).await
}

/// Core logic to display a VOICEVOX speaker selection page
async fn show_voicevox_page(
    ctx: &Context,
    interaction: &ComponentInteraction,
    page: usize,
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

    // Get VOICEVOX speakers
    let tts_client = data_read
        .get::<TTSClientData>()
        .ok_or_else(|| NCBError::config("Cannot get TTSClientData"))?;

    let voicevox_speakers = tts_client
        .voicevox_client
        .get_styles()
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to get VOICEVOX styles: {}", e);
            vec![("VOICEVOX API unavailable".to_string(), 1)]
        });

    let voicevox_speaker = config.voicevox_speaker.unwrap_or(1);

    // Calculate pagination
    let total_speakers = voicevox_speakers.len();
    let total_pages = (total_speakers + SPEAKERS_PER_PAGE - 1) / SPEAKERS_PER_PAGE;
    let current_page = page.min(total_pages.saturating_sub(1));

    // Get speakers for current page
    let start_idx = current_page * SPEAKERS_PER_PAGE;
    let end_idx = (start_idx + SPEAKERS_PER_PAGE).min(total_speakers);
    let page_speakers = &voicevox_speakers[start_idx..end_idx];

    // Build components
    let mut components = Vec::new();

    // Speaker select menu for current page
    let mut options = Vec::new();
    for (name, id) in page_speakers {
        options.push(
            CreateSelectMenuOption::new(
                name,
                format!("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_{}", id),
            )
            .default_selection(*id == voicevox_speaker),
        );
    }

    components.push(CreateActionRow::SelectMenu(
        CreateSelectMenu::new(
            format!("TTS_CONFIG_VOICEVOX_SPEAKER_PAGE_{}", current_page),
            CreateSelectMenuKind::String { options },
        )
        .placeholder(format!("VOICEVOX Speaker (Page {}/{})", current_page + 1, total_pages)),
    ));

    // Pagination buttons
    let mut pagination_buttons = Vec::new();

    // Previous page button
    if current_page > 0 {
        pagination_buttons.push(
            CreateButton::new(format!("TTS_CONFIG_VOICEVOX_PAGE_{}", current_page - 1))
                .label("◀ 前のページ")
                .style(ButtonStyle::Primary),
        );
    }

    // Page indicator (disabled button showing current page)
    pagination_buttons.push(
        CreateButton::new("TTS_CONFIG_VOICEVOX_PAGE_INDICATOR")
            .label(format!("Page {}/{}", current_page + 1, total_pages))
            .style(ButtonStyle::Secondary)
            .disabled(true),
    );

    // Next page button
    if current_page < total_pages - 1 {
        pagination_buttons.push(
            CreateButton::new(format!("TTS_CONFIG_VOICEVOX_PAGE_{}", current_page + 1))
                .label("次のページ ▶")
                .style(ButtonStyle::Primary),
        );
    }

    if pagination_buttons.len() > 1 {
        // Only add pagination row if there are actual navigation buttons
        components.push(CreateActionRow::Buttons(pagination_buttons));
    }

    // Back to main config button
    components.push(CreateActionRow::Buttons(vec![CreateButton::new(
        TTS_CONFIG_BACK_TO_MAIN,
    )
    .label("設定に戻る")
    .style(ButtonStyle::Secondary)]));

    // Update the interaction message
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content(format!(
                        "VOICEVOX設定 (Page {}/{}, {} speakers)",
                        current_page + 1,
                        total_pages,
                        total_speakers
                    ))
                    .components(components),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;

    Ok(())
}
