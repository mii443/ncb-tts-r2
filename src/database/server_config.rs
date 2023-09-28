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
}
