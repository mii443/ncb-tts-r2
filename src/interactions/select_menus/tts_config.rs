use crate::{
    errors::{NCBError, Result},
    interactions::utils,
    tts::tts_type::TTSType,
};
use serenity::{
    all::{
        ComponentInteraction, ComponentInteractionDataKind, CreateInteractionResponse,
        CreateInteractionResponseMessage,
    },
    prelude::Context,
};

pub async fn handle_tts_config_select(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let ComponentInteractionDataKind::StringSelect { values, .. } = &interaction.data.kind else {
        return Ok(());
    };
    let Some(selected) = values.first() else {
        return Ok(());
    };
    let engine = match selected.as_str() {
        "TTS_CONFIG_ENGINE_SELECTED_GOOGLE" => Some(TTSType::GCP),
        "TTS_CONFIG_ENGINE_SELECTED_VOICEVOX" => Some(TTSType::VOICEVOX),
        #[cfg(toriel_voice)]
        "TTS_CONFIG_ENGINE_SELECTED_TORIEL" => Some(TTSType::TORIEL),
        _ => None,
    };
    let speaker = selected
        .strip_prefix("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_")
        .map(|id| {
            id.parse::<i64>()
                .map_err(|_| NCBError::invalid_input("Invalid speaker ID format"))
        })
        .transpose()?;
    if engine.is_none() && speaker.is_none() {
        return Ok(());
    }
    let config = utils::update_user_config(ctx, interaction.user.id.get(), |config| {
        if let Some(engine) = &engine {
            config.tts_type = Some(engine.clone());
        }
        if let Some(speaker) = speaker {
            config.voicevox_speaker = Some(speaker);
        }
        Ok(())
    })
    .await?;
    let content = if speaker.is_some() && config.tts_type.unwrap_or(TTSType::GCP) == TTSType::GCP {
        "設定しました\nこの音声を使うにはAPIをGoogleからVOICEVOXに変更する必要があります。"
    } else {
        "設定しました"
    };
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(content)
                    .ephemeral(true),
            ),
        )
        .await?;
    Ok(())
}
