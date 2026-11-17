use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TTSType {
    GCP,
    VOICEVOX,
    TORIEL,
}

impl TTSType {
    pub fn available_or_default(self) -> Self {
        #[cfg(not(toriel_voice))]
        if self == Self::TORIEL {
            return Self::GCP;
        }

        self
    }
}

#[cfg(test)]
mod tests {
    use super::TTSType;

    #[test]
    fn unavailable_engine_falls_back_to_gcp() {
        let expected = if cfg!(toriel_voice) {
            TTSType::TORIEL
        } else {
            TTSType::GCP
        };

        assert_eq!(TTSType::TORIEL.available_or_default(), expected);
    }
}
