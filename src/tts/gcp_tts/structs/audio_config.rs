use serde::{Serialize, Deserialize};

/// Example:
/// ```rust
/// AudioConfig {
///     audioEncoding: String::from("mp3"),
///     speakingRate: 1.2f32,
///     pitch: 1.0f32
/// }
/// ```
#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct AudioConfig {
    pub audioEncoding: String,
    pub speakingRate: f32,
    pub pitch: f32
}