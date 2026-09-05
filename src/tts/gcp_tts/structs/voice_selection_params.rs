use serde::{Deserialize, Serialize};

/// Example:
/// ```rust
/// use ncb_tts_r2::tts::gcp_tts::structs::voice_selection_params::VoiceSelectionParams;
///
/// VoiceSelectionParams {
///     languageCode: String::from("ja-JP"),
///     name: String::from("ja-JP-Wavenet-B"),
///     ssmlGender: String::from("neutral")
/// };
/// ```
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[allow(non_snake_case)]
pub struct VoiceSelectionParams {
    pub languageCode: String,
    pub name: String,
    pub ssmlGender: String,
}
