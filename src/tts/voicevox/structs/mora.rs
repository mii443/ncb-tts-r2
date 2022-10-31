use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Mora {
    pub text: String,
    pub consonant: Option<String>,
    pub consonant_length: Option<f64>,
    pub vowel: String,
    pub vowel_length: f64,
    pub pitch: f64,
}
