use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Speaker {
    pub supported_features: SupportedFeatures,
    pub name: String,
    pub speaker_uuid: String,
    pub styles: Vec<Style>,
    pub version: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SupportedFeatures {
    pub permitted_synthesis_morphing: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Style {
    pub name: String,
    pub id: i64,
}
