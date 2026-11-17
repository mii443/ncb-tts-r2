use serenity::{
    all::{
        ButtonStyle, CommandInteraction, CreateActionRow, CreateButton, CreateComponent,
        CreateInteractionResponse, CreateInteractionResponseMessage,
    },
    prelude::Context,
};

use crate::{data::UserData, interactions::utils, tts::tts_type::TTSType};

#[tracing::instrument(skip_all)]
pub async fn config_command(
    ctx: &Context,
    command: &CommandInteraction,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = ctx.data::<UserData>();

    let config = data
        .database
        .get_user_config_or_default(command.user.id.get())
        .await?
        .ok_or_else(|| crate::errors::NCBError::config("User config not found"))?;

    let tts_type = config
        .tts_type
        .unwrap_or(TTSType::GCP)
        .available_or_default();

    let engine_select = utils::build_tts_engine_select(tts_type);

    let voicevox_button = CreateActionRow::Buttons(
        vec![CreateButton::new("TTS_CONFIG_VOICEVOX")
            .label("VOICEVOX設定")
            .style(ButtonStyle::Primary)]
        .into(),
    );

    let mut components: Vec<CreateComponent> = vec![
        CreateComponent::ActionRow(engine_select),
        CreateComponent::ActionRow(voicevox_button),
    ];

    let server_button = CreateActionRow::Buttons(
        vec![CreateButton::new("TTS_CONFIG_SERVER")
            .label("サーバー設定")
            .style(ButtonStyle::Primary)]
        .into(),
    );

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
