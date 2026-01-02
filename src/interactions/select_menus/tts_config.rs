//! TTS configuration select menu handler
//!
//! Handles TTS engine and VOICEVOX speaker selection

use crate::{errors::Result, interactions::utils, tts::tts_type::TTSType};
use serenity::{
    all::{
        ComponentInteraction, ComponentInteractionDataKind, CreateInteractionResponse,
        CreateInteractionResponseMessage,
    },
    prelude::Context,
};

/// Handle TTS engine and VOICEVOX speaker selection
pub async fn handle_tts_config_select(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    if let ComponentInteractionDataKind::StringSelect { ref values, .. } = interaction.data.kind {
        if values.is_empty() {
            return Ok(());
        }

        let selected = &values[0];
        let mut config = utils::get_user_config(ctx, interaction.user.id.get()).await?;
        let mut config_changed = false;
        let mut voicevox_changed = false;

        match selected.as_str() {
            "TTS_CONFIG_ENGINE_SELECTED_GOOGLE" => {
                config.tts_type = Some(TTSType::GCP);
                config_changed = true;
            }
            "TTS_CONFIG_ENGINE_SELECTED_VOICEVOX" => {
                config.tts_type = Some(TTSType::VOICEVOX);
                config_changed = true;
            }
            "TTS_CONFIG_ENGINE_SELECTED_TORIEL" => {
                config.tts_type = Some(TTSType::TORIEL);
                config_changed = true;
            }
            _ => {
                if selected.starts_with("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_") {
                    let speaker_id = selected
                        .strip_prefix("TTS_CONFIG_VOICEVOX_SPEAKER_SELECTED_")
                        .and_then(|id_str| id_str.parse::<i64>().ok())
                        .ok_or_else(|| {
                            crate::errors::NCBError::invalid_input("Invalid speaker ID format")
                        })?;

                    config.voicevox_speaker = Some(speaker_id);
                    config_changed = true;
                    voicevox_changed = true;
                }
            }
        }

        if config_changed {
            utils::set_user_config(ctx, interaction.user.id.get(), config.clone()).await?;

            let response_content = if voicevox_changed
                && config.tts_type.unwrap_or(TTSType::GCP) == TTSType::GCP
            {
                "設定しました\nこの音声を使うにはAPIをGoogleからVOICEVOXに変更する必要があります。"
            } else {
                "設定しました"
            };

            interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(response_content)
                            .ephemeral(true),
                    ),
                )
                .await
                .map_err(|e| crate::errors::NCBError::Discord(e))?;
        }
    }

    Ok(())
}
