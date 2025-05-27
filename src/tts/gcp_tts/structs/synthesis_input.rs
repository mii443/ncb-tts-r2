use serde::{Deserialize, Serialize};

/// Example:
/// ```rust
/// use ncb_tts_r2::tts::gcp_tts::structs::synthesis_input::SynthesisInput;
///
/// SynthesisInput {
///     text: None,
///     ssml: Some(String::from("<speak>test</speak>"))
/// };
/// ```
#[derive(Serialize, Deserialize, Debug, Hash, PartialEq, Eq, Clone)]
pub struct SynthesisInput {
    pub text: Option<String>,
    pub ssml: Option<String>,
}
