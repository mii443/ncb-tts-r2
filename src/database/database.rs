use crate::tts::{gcp_tts::structs::voice_selection_params::VoiceSelectionParams, tts_type::TTSType};

use super::user_config::UserConfig;
use redis::Commands;

pub struct Database {
    pub connection: redis::Connection
}

impl Database {
    pub fn new(connection: redis::Connection) -> Self {
        Self { connection }
    }

    pub async fn get_user_config(&mut self, user_id: u64) -> redis::RedisResult<Option<UserConfig>> {
        let config: String = self.connection.get(format!("discord_user:{}", user_id)).unwrap_or_default();

        match serde_json::from_str(&config) {
            Ok(config) => Ok(Some(config)),
            Err(_) => Ok(None)
        }
    }

    pub async fn set_user_config(&mut self, user_id: u64, config: UserConfig) -> redis::RedisResult<()> {
        let config = serde_json::to_string(&config).unwrap();
        self.connection.set::<String, String, ()>(format!("discord_user:{}", user_id), config).unwrap();
        Ok(())
    }

    pub async fn set_default_user_config(&mut self, user_id: u64) -> redis::RedisResult<()> {
        let voice_selection = VoiceSelectionParams {
            languageCode: String::from("ja-JP"),
            name: String::from("ja-JP-Wavenet-B"),
            ssmlGender: String::from("neutral")
        };

        let voice_type = TTSType::GCP;

        let config = UserConfig {
            tts_type: Some(voice_type),
            gcp_tts_voice: Some(voice_selection),
            voicevox_speaker: Some(1)
        };

        self.connection.set(format!("discord_user:{}", user_id), serde_json::to_string(&config).unwrap())?;

        Ok(())
    }

    pub async fn get_user_config_or_default(&mut self, user_id: u64) -> redis::RedisResult<Option<UserConfig>> {
        let config = self.get_user_config(user_id).await?;
        match config {
            Some(_) => Ok(config),
            None => {
                self.set_default_user_config(user_id).await?;
                self.get_user_config(user_id).await
            }
        }
    }
}
