use serenity::{
    all::{
        ButtonStyle, CommandInteraction, CreateActionRow, CreateButton, CreateComponent,
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateSelectMenu,
        CreateSelectMenuKind, CreateSelectMenuOption,
    },
    prelude::Context,
};

use crate::{data::UserData, tts::tts_type::TTSType};

#[tracing::instrument(skip_all)]
pub async fn config_command(
    ctx: &Context,
    command: &CommandInteraction,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = ctx.data::<UserData>();

    let config = data.database
        .get_user_config_or_default(command.user.id.get())
        .await
        .unwrap()
        .unwrap();

    let tts_type = config.tts_type.unwrap_or(TTSType::GCP);

    let engine_select = CreateActionRow::SelectMenu(
        CreateSelectMenu::new(
            "TTS_CONFIG_ENGINE",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("Google TTS", "TTS_CONFIG_ENGINE_SELECTED_GOOGLE")
                        .default_selection(tts_type == TTSType::GCP),
                    CreateSelectMenuOption::new("VOICEVOX", "TTS_CONFIG_ENGINE_SELECTED_VOICEVOX")
                        .default_selection(tts_type == TTSType::VOICEVOX),
                    CreateSelectMenuOption::new("Toriel", "TTS_CONFIG_ENGINE_SELECTED_TORIEL")
                        .default_selection(tts_type == TTSType::TORIEL),
                ].into(),
            },
        )
        .placeholder("読み上げAPIを選択"),
    );

    let voicevox_button = CreateActionRow::Buttons(vec![CreateButton::new("TTS_CONFIG_VOICEVOX")
        .label("VOICEVOX設定")
        .style(ButtonStyle::Primary)].into());

    let mut components: Vec<CreateComponent> = vec![
        CreateComponent::ActionRow(engine_select),
        CreateComponent::ActionRow(voicevox_button),
    ];

    let server_button = CreateActionRow::Buttons(vec![CreateButton::new("TTS_CONFIG_SERVER")
        .label("サーバー設定")
        .style(ButtonStyle::Primary)].into());

    components.push(CreateComponent::ActionRow(server_button));

    command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("読み上げ設定")
                    .components(components)
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}
