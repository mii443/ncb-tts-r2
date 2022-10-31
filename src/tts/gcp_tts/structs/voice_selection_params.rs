use serde::{Deserialize, Serialize};

/// Example:
/// ```rust
/// VoiceSelectionParams {
///     languageCode: String::from("ja-JP"),
///     name: String::from("ja-JP-Wavenet-B"),
///     ssmlGender: String::from("neutral")
/// }
/// ```
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[allow(non_snake_case)]
pub struct VoiceSelectionParams {
    pub languageCode: String,
    pub name: String,
    pub ssmlGender: String,
}
