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
