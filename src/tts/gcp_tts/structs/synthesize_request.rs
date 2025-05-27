use crate::tts::gcp_tts::structs::{
    audio_config::AudioConfig, synthesis_input::SynthesisInput,
    voice_selection_params::VoiceSelectionParams,
};
use serde::{Deserialize, Serialize};

/// Example:
/// ```rust
/// SynthesizeRequest {
///     input: SynthesisInput {
///         text: None,
///         ssml: Some(String::from("<speak>test</speak>"))
///     },
///     voice: VoiceSelectionParams {
///         languageCode: String::from("ja-JP"),
///         name: String::from("ja-JP-Wavenet-B"),
///         ssmlGender: String::from("neutral")
///     },
///     audioConfig: AudioConfig {
///         audioEncoding: String::from("mp3"),
///         speakingRate: 1.2f32,
///         pitch: 1.0f32
///     }
/// }
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(non_snake_case)]
pub struct SynthesizeRequest {
    pub input: SynthesisInput,
    pub voice: VoiceSelectionParams,
    pub audioConfig: AudioConfig,
}
