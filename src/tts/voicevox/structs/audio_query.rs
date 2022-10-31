use serde::{Deserialize, Serialize};

use super::accent_phrase::AccentPhrase;

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AudioQuery {
    pub accent_phrases: Vec<AccentPhrase>,
    pub speedScale: f64,
    pub pitchScale: f64,
    pub intonationScale: f64,
    pub volumeScale: f64,
    pub prePhonemeLength: f64,
    pub postPhonemeLength: f64,
    pub outputSamplingRate: f64,
    pub outputStereo: bool,
    pub kana: Option<String>,
}
