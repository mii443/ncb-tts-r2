use super::dictionary::Dictionary;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DictionaryOnlyServerConfig {
    pub dictionary: Dictionary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub dictionary: Dictionary,
    pub autostart_channel_id: Option<u64>,
    pub autostart_text_channel_id: Option<u64>,
    pub voice_state_announce: Option<bool>,
    pub read_username: Option<bool>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            dictionary: Dictionary::new(),
            autostart_channel_id: None,
            autostart_text_channel_id: None,
            voice_state_announce: Some(false),
            read_username: Some(false),
        }
    }
}
