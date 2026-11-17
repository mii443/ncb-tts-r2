use serde::{Deserialize, Serialize};

use crate::tts::{
    gcp_tts::structs::voice_selection_params::VoiceSelectionParams, tts_type::TTSType,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserConfig {
    pub tts_type: Option<TTSType>,
    pub gcp_tts_voice: Option<VoiceSelectionParams>,
    pub voicevox_speaker: Option<i64>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            tts_type: Some(TTSType::GCP),
            voicevox_speaker: Some(crate::errors::constants::DEFAULT_VOICEVOX_SPEAKER),
            gcp_tts_voice: Some(VoiceSelectionParams {
                languageCode: "ja-JP".into(),
                name: "ja-JP-Wavenet-B".into(),
                ssmlGender: "neutral".into(),
            }),
        }
    }
}
