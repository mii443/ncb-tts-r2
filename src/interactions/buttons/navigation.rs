use crate::{
    data::UserData,
    errors::{constants::*, NCBError, Result},
    interactions::utils,
    tts::tts_type::TTSType,
};
use serenity::{
    all::{
        ButtonStyle, ComponentInteraction, CreateActionRow, CreateButton, CreateComponent,
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateSelectMenu,
        CreateSelectMenuKind, CreateSelectMenuOption,
    },
    prelude::Context,
};

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

pub async fn handle_back_to_server_config(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    handle_show_server_config(ctx, interaction).await
}

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

pub async fn handle_back_to_main_config(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let data = ctx.data::<UserData>();

    let config = data.database
        .get_user_config_or_default(interaction.user.id.get())
        .await
        .map_err(|e| NCBError::database(format!("Failed to get user config: {}", e)))?
        .ok_or_else(|| NCBError::config("User config not found"))?;

    let tts_type = config.tts_type.unwrap_or(TTSType::GCP);

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
                ].into(),
            },
        )
        .placeholder("読み上げAPIを選択"),
    );

    let voicevox_button = CreateActionRow::Buttons(vec![CreateButton::new(TTS_CONFIG_VOICEVOX)
        .label("VOICEVOX設定")
        .style(ButtonStyle::Primary)].into());

    let server_button = CreateActionRow::Buttons(vec![CreateButton::new(TTS_CONFIG_SERVER)
        .label("サーバー設定")
        .style(ButtonStyle::Primary)].into());

    let components: Vec<CreateComponent> = vec![
        CreateComponent::ActionRow(engine_select),
        CreateComponent::ActionRow(voicevox_button),
        CreateComponent::ActionRow(server_button),
    ];

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
