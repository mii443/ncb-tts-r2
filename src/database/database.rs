use std::fmt::Debug;

use crate::tts::{
    gcp_tts::structs::voice_selection_params::VoiceSelectionParams, tts_type::TTSType,
};

use super::{dictionary::Dictionary, server_config::ServerConfig, user_config::UserConfig};
use redis::Commands;

#[derive(Debug, Clone)]
pub struct Database {
    pub client: redis::Client,
}

impl Database {
    pub fn new(client: redis::Client) -> Self {
        Self { client }
    }

    fn server_key(server_id: u64) -> String {
        format!("discord_server:{}", server_id)
    }

    fn user_key(user_id: u64) -> String {
        format!("discord_user:{}", user_id)
    }

    #[tracing::instrument]
    fn get_config<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> redis::RedisResult<Option<T>> {
        match self.client.get_connection() {
            Ok(mut connection) => {
                let config: String = connection.get(key).unwrap_or_default();

                if config.is_empty() {
                    return Ok(None);
                }

                match serde_json::from_str(&config) {
                    Ok(config) => Ok(Some(config)),
                    Err(_) => Ok(None),
                }
            }
            Err(e) => Err(e),
        }
    }

    #[tracing::instrument]
    fn set_config<T: serde::Serialize + Debug>(
        &self,
        key: &str,
        config: &T,
    ) -> redis::RedisResult<()> {
        match self.client.get_connection() {
            Ok(mut connection) => {
                let config_str = serde_json::to_string(config).unwrap();
                connection.set::<_, _, ()>(key, config_str)
            }
            Err(e) => Err(e),
        }
    }

    #[tracing::instrument]
    pub async fn get_server_config(
        &self,
        server_id: u64,
    ) -> redis::RedisResult<Option<ServerConfig>> {
        self.get_config(&Self::server_key(server_id))
    }

    #[tracing::instrument]
    pub async fn get_user_config(&self, user_id: u64) -> redis::RedisResult<Option<UserConfig>> {
        self.get_config(&Self::user_key(user_id))
    }

    #[tracing::instrument]
    pub async fn set_server_config(
        &self,
        server_id: u64,
        config: ServerConfig,
    ) -> redis::RedisResult<()> {
        self.set_config(&Self::server_key(server_id), &config)
    }

    #[tracing::instrument]
    pub async fn set_user_config(
        &self,
        user_id: u64,
        config: UserConfig,
    ) -> redis::RedisResult<()> {
        self.set_config(&Self::user_key(user_id), &config)
    }

    #[tracing::instrument]
    pub async fn set_default_server_config(&self, server_id: u64) -> redis::RedisResult<()> {
        let config = ServerConfig {
            dictionary: Dictionary::new(),
            autostart_channel_id: None,
        };

        self.set_server_config(server_id, config).await
    }

    #[tracing::instrument]
    pub async fn set_default_user_config(&self, user_id: u64) -> redis::RedisResult<()> {
        let voice_selection = VoiceSelectionParams {
            languageCode: String::from("ja-JP"),
            name: String::from("ja-JP-Wavenet-B"),
            ssmlGender: String::from("neutral"),
        };

        let config = UserConfig {
            tts_type: Some(TTSType::GCP),
            gcp_tts_voice: Some(voice_selection),
            voicevox_speaker: Some(1),
        };

        self.set_user_config(user_id, config).await
    }

    #[tracing::instrument]
    pub async fn get_server_config_or_default(
        &self,
        server_id: u64,
    ) -> redis::RedisResult<Option<ServerConfig>> {
        match self.get_server_config(server_id).await? {
            Some(config) => Ok(Some(config)),
            None => {
                self.set_default_server_config(server_id).await?;
                self.get_server_config(server_id).await
            }
        }
    }

    #[tracing::instrument]
    pub async fn get_user_config_or_default(
        &self,
        user_id: u64,
    ) -> redis::RedisResult<Option<UserConfig>> {
        match self.get_user_config(user_id).await? {
            Some(config) => Ok(Some(config)),
            None => {
                self.set_default_user_config(user_id).await?;
                self.get_user_config(user_id).await
            }
        }
    }
}
