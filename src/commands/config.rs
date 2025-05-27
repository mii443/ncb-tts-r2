use serenity::{
    all::{
        ButtonStyle, CommandInteraction, CreateActionRow, CreateButton, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateSelectMenu, CreateSelectMenuKind,
        CreateSelectMenuOption,
    },
    prelude::Context,
};

use crate::{
    data::{DatabaseClientData, TTSClientData},
    tts::tts_type::TTSType,
};

#[tracing::instrument]
pub async fn config_command(
    ctx: &Context,
    command: &CommandInteraction,
) -> Result<(), Box<dyn std::error::Error>> {
    let data_read = ctx.data.read().await;

    let config = {
        let database = data_read
            .get::<DatabaseClientData>()
            .expect("Cannot get DatabaseClientData")
            .clone();
        database
            .get_user_config_or_default(command.user.id.get())
            .await
            .unwrap()
            .unwrap()
    };

    let tts_client = data_read
        .get::<TTSClientData>()
        .expect("Cannot get TTSClientData");
    let voicevox_speakers = tts_client.voicevox_client.get_styles().await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to get VOICEVOX styles: {}", e);
            vec![("VOICEVOX API unavailable".to_string(), 1)]
        });

    let voicevox_speaker = config.voicevox_speaker.unwrap_or(1);
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
                ],
            },
        )
        .placeholder("読み上げAPIを選択"),
    );

    let server_button = CreateActionRow::Buttons(vec![CreateButton::new("TTS_CONFIG_SERVER")
        .label("サーバー設定")
        .style(ButtonStyle::Primary)]);

    let mut components = vec![engine_select, server_button];

    for (index, speaker_chunk) in voicevox_speakers[0..24].chunks(25).enumerate() {
        let mut options = Vec::new();

        for (name, id) in speaker_chunk {
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
                format!("TTS_CONFIG_VOICEVOX_SPEAKER_{}", index),
                CreateSelectMenuKind::String { options },
            )
            .placeholder("VOICEVOX Speakerを指定"),
        ));
    }

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
