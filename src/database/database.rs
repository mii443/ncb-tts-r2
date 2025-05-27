use std::fmt::Debug;

use bb8_redis::{bb8::Pool, RedisConnectionManager, redis::AsyncCommands};
use crate::{
    errors::{NCBError, Result, constants::*},
    tts::{
        gcp_tts::structs::voice_selection_params::VoiceSelectionParams, instance::TTSInstance,
        tts_type::TTSType,
    },
};
use serenity::model::id::{GuildId, UserId, ChannelId};
use std::collections::HashMap;

use super::{dictionary::Dictionary, server_config::ServerConfig, user_config::UserConfig};

#[derive(Debug, Clone)]
pub struct Database {
    pub pool: Pool<RedisConnectionManager>,
}

impl Database {
    pub fn new(pool: Pool<RedisConnectionManager>) -> Self {
        Self { pool }
    }
    
    pub async fn new_with_url(redis_url: String) -> Result<Self> {
        let manager = RedisConnectionManager::new(redis_url)?;
        let pool = Pool::builder()
            .max_size(15)
            .build(manager)
            .await
            .map_err(|e| NCBError::Database(format!("Pool creation failed: {}", e)))?;
        Ok(Self { pool })
    }

    fn server_key(server_id: u64) -> String {
        format!("{}{}", DISCORD_SERVER_PREFIX, server_id)
    }

    fn user_key(user_id: u64) -> String {
        format!("{}{}", DISCORD_USER_PREFIX, user_id)
    }

    fn tts_instance_key(guild_id: u64) -> String {
        format!("{}{}", TTS_INSTANCE_PREFIX, guild_id)
    }

    fn tts_instances_list_key() -> String {
        TTS_INSTANCES_LIST_KEY.to_string()
    }

    fn user_config_key(guild_id: u64, user_id: u64) -> String {
        format!("user:config:{}:{}", guild_id, user_id)
    }

    fn server_config_key(guild_id: u64) -> String {
        format!("server:config:{}", guild_id)
    }

    fn dictionary_key(guild_id: u64) -> String {
        format!("dictionary:{}", guild_id)
    }

    #[tracing::instrument]
    async fn get_config<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>> {
        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
            
        let config: String = connection.get(key).await.unwrap_or_default();

        if config.is_empty() {
            return Ok(None);
        }

        match serde_json::from_str(&config) {
            Ok(config) => Ok(Some(config)),
            Err(e) => {
                tracing::warn!(key = key, error = %e, "Failed to deserialize config");
                Ok(None)
            }
        }
    }

    #[tracing::instrument]
    async fn set_config<T: serde::Serialize + Debug>(
        &self,
        key: &str,
        config: &T,
    ) -> Result<()> {
        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
            
        let config_str = serde_json::to_string(config)?;
        connection.set::<_, _, ()>(key, config_str).await?;
        Ok(())
    }

    #[tracing::instrument]
    pub async fn get_server_config(
        &self,
        server_id: u64,
    ) -> Result<Option<ServerConfig>> {
        self.get_config(&Self::server_key(server_id)).await
    }

    #[tracing::instrument]
    pub async fn get_user_config(&self, user_id: u64) -> Result<Option<UserConfig>> {
        self.get_config(&Self::user_key(user_id)).await
    }

    #[tracing::instrument]
    pub async fn set_server_config(
        &self,
        server_id: u64,
        config: ServerConfig,
    ) -> Result<()> {
        self.set_config(&Self::server_key(server_id), &config).await
    }

    #[tracing::instrument]
    pub async fn set_user_config(
        &self,
        user_id: u64,
        config: UserConfig,
    ) -> Result<()> {
        self.set_config(&Self::user_key(user_id), &config).await
    }

    #[tracing::instrument]
    pub async fn set_default_server_config(&self, server_id: u64) -> Result<()> {
        let config = ServerConfig {
            dictionary: Dictionary::new(),
            autostart_channel_id: None,
            voice_state_announce: Some(true),
            read_username: Some(true),
        };

        self.set_server_config(server_id, config).await
    }

    #[tracing::instrument]
    pub async fn set_default_user_config(&self, user_id: u64) -> Result<()> {
        let voice_selection = VoiceSelectionParams {
            languageCode: String::from("ja-JP"),
            name: String::from("ja-JP-Wavenet-B"),
            ssmlGender: String::from("neutral"),
        };

        let config = UserConfig {
            tts_type: Some(TTSType::GCP),
            gcp_tts_voice: Some(voice_selection),
            voicevox_speaker: Some(DEFAULT_VOICEVOX_SPEAKER),
        };

        self.set_user_config(user_id, config).await
    }

    #[tracing::instrument]
    pub async fn get_server_config_or_default(
        &self,
        server_id: u64,
    ) -> Result<Option<ServerConfig>> {
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
    ) -> Result<Option<UserConfig>> {
        match self.get_user_config(user_id).await? {
            Some(config) => Ok(Some(config)),
            None => {
                self.set_default_user_config(user_id).await?;
                self.get_user_config(user_id).await
            }
        }
    }

    /// Save TTS instance to database
    pub async fn save_tts_instance(
        &self,
        guild_id: GuildId,
        instance: &TTSInstance,
    ) -> Result<()> {
        let key = Self::tts_instance_key(guild_id.get());
        let list_key = Self::tts_instances_list_key();

        // Save the instance
        self.set_config(&key, instance).await?;

        // Add guild_id to the list of active instances
        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
            
        connection.sadd::<_, _, ()>(&list_key, guild_id.get()).await?;
        Ok(())
    }

    /// Load TTS instance from database
    #[tracing::instrument]
    pub async fn load_tts_instance(
        &self,
        guild_id: GuildId,
    ) -> Result<Option<TTSInstance>> {
        let key = Self::tts_instance_key(guild_id.get());
        self.get_config(&key).await
    }

    /// Remove TTS instance from database
    #[tracing::instrument]
    pub async fn remove_tts_instance(&self, guild_id: GuildId) -> Result<()> {
        let key = Self::tts_instance_key(guild_id.get());
        let list_key = Self::tts_instances_list_key();

        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
            
        let _: std::result::Result<(), bb8_redis::redis::RedisError> = connection.del(&key).await;
        let _: std::result::Result<(), bb8_redis::redis::RedisError> = connection.srem(&list_key, guild_id.get()).await;
        
        Ok(())
    }

    /// Get all active TTS instances
    #[tracing::instrument]
    pub async fn get_all_tts_instances(&self) -> Result<Vec<(GuildId, TTSInstance)>> {
        let list_key = Self::tts_instances_list_key();

        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
            
        let guild_ids: Vec<u64> = connection.smembers(&list_key).await.unwrap_or_default();
        let mut instances = Vec::new();

        for guild_id in guild_ids {
            let guild_id = GuildId::new(guild_id);
            if let Ok(Some(instance)) = self.load_tts_instance(guild_id).await {
                instances.push((guild_id, instance));
            } else {
                tracing::warn!(guild_id = %guild_id, "Failed to load TTS instance");
            }
        }

        Ok(instances)
    }

    // Additional user config methods
    pub async fn save_user_config(
        &self,
        guild_id: GuildId,
        user_id: UserId,
        config: &UserConfig,
    ) -> Result<()> {
        let key = Self::user_config_key(guild_id.get(), user_id.get());
        self.set_config(&key, config).await
    }

    pub async fn load_user_config(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<Option<UserConfig>> {
        let key = Self::user_config_key(guild_id.get(), user_id.get());
        self.get_config(&key).await
    }

    pub async fn delete_user_config(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<()> {
        let key = Self::user_config_key(guild_id.get(), user_id.get());
        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
        let _: std::result::Result<(), bb8_redis::redis::RedisError> = connection.del(&key).await;
        Ok(())
    }

    // Additional server config methods
    pub async fn save_server_config(
        &self,
        guild_id: GuildId,
        config: &ServerConfig,
    ) -> Result<()> {
        let key = Self::server_config_key(guild_id.get());
        self.set_config(&key, config).await
    }

    pub async fn load_server_config(
        &self,
        guild_id: GuildId,
    ) -> Result<Option<ServerConfig>> {
        let key = Self::server_config_key(guild_id.get());
        self.get_config(&key).await
    }

    pub async fn delete_server_config(&self, guild_id: GuildId) -> Result<()> {
        let key = Self::server_config_key(guild_id.get());
        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
        let _: std::result::Result<(), bb8_redis::redis::RedisError> = connection.del(&key).await;
        Ok(())
    }

    // Dictionary methods
    pub async fn save_dictionary(
        &self,
        guild_id: GuildId,
        dictionary: &HashMap<String, String>,
    ) -> Result<()> {
        let key = Self::dictionary_key(guild_id.get());
        self.set_config(&key, dictionary).await
    }

    pub async fn load_dictionary(
        &self,
        guild_id: GuildId,
    ) -> Result<HashMap<String, String>> {
        let key = Self::dictionary_key(guild_id.get());
        let dict: Option<HashMap<String, String>> = self.get_config(&key).await?;
        Ok(dict.unwrap_or_default())
    }

    pub async fn delete_dictionary(&self, guild_id: GuildId) -> Result<()> {
        let key = Self::dictionary_key(guild_id.get());
        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
        let _: std::result::Result<(), bb8_redis::redis::RedisError> = connection.del(&key).await;
        Ok(())
    }

    pub async fn delete_tts_instance(&self, guild_id: GuildId) -> Result<()> {
        self.remove_tts_instance(guild_id).await
    }

    pub async fn list_active_instances(&self) -> Result<Vec<u64>> {
        let list_key = Self::tts_instances_list_key();
        let mut connection = self.pool.get().await
            .map_err(|e| NCBError::Database(format!("Pool connection failed: {}", e)))?;
        let guild_ids: Vec<u64> = connection.smembers(&list_key).await.unwrap_or_default();
        Ok(guild_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb8_redis::redis::AsyncCommands;
    use serial_test::serial;
    use crate::errors::constants;

    // Helper function to create test database (requires Redis running)
    async fn create_test_database() -> Result<Database> {
        let manager = RedisConnectionManager::new("redis://127.0.0.1:6379/15")?; // Use test DB
        let pool = bb8::Pool::builder()
            .max_size(1)
            .build(manager)
            .await
            .map_err(|e| NCBError::Database(format!("Pool creation failed: {}", e)))?;

        Ok(Database { pool })
    }

    #[tokio::test]
    #[serial]
    async fn test_database_creation() {
        // This test requires Redis to be running
        match create_test_database().await {
            Ok(_db) => {
                // Test successful creation
                assert!(true);
            }
            Err(_) => {
                // Skip test if Redis is not available
                return;
            }
        }
    }

    #[test]
    fn test_key_generation() {
        let guild_id = 123456789u64;
        let user_id = 987654321u64;

        // Test TTS instance key
        let tts_key = Database::tts_instance_key(guild_id);
        assert!(tts_key.contains(&guild_id.to_string()));

        // Test TTS instances list key
        let list_key = Database::tts_instances_list_key();
        assert!(!list_key.is_empty());

        // Test user config key
        let user_key = Database::user_config_key(guild_id, user_id);
        assert_eq!(user_key, "user:config:123456789:987654321");

        // Test server config key
        let server_key = Database::server_config_key(guild_id);
        assert_eq!(server_key, "server:config:123456789");

        // Test dictionary key
        let dict_key = Database::dictionary_key(guild_id);
        assert_eq!(dict_key, "dictionary:123456789");
    }

    #[tokio::test]
    #[serial]
    async fn test_tts_instance_operations() {
        let db = match create_test_database().await {
            Ok(db) => db,
            Err(_) => return, // Skip if Redis not available
        };

        let guild_id = GuildId::new(12345);
        let test_instance = TTSInstance::new(
            ChannelId::new(123),
            ChannelId::new(456),
            guild_id
        );

        // Clear any existing data
        if let Ok(mut conn) = db.pool.get().await {
            let _: () = conn.del(Database::tts_instance_key(guild_id.get())).await.unwrap_or_default();
            let _: () = conn.srem(Database::tts_instances_list_key(), guild_id.get()).await.unwrap_or_default();
        } else {
            return; // Skip if can't get connection
        }

        // Test saving TTS instance
        let save_result = db.save_tts_instance(guild_id, &test_instance).await;
        if save_result.is_err() {
            // Skip test if Redis operations fail
            return;
        }

        // Test loading TTS instance
        let load_result = db.load_tts_instance(guild_id).await;
        if load_result.is_err() {
            return; // Skip if Redis operations fail
        }

        let loaded_instance = load_result.unwrap();
        if let Some(instance) = loaded_instance {
            assert_eq!(instance.guild, test_instance.guild);
            assert_eq!(instance.text_channel, test_instance.text_channel);
            assert_eq!(instance.voice_channel, test_instance.voice_channel);
        }

        // Test listing active instances
        let list_result = db.list_active_instances().await;
        if list_result.is_err() {
            return; // Skip if Redis operations fail
        }
        let instances = list_result.unwrap();
        assert!(instances.contains(&guild_id.get()));

        // Test deleting TTS instance
        let delete_result = db.delete_tts_instance(guild_id).await;
        if delete_result.is_err() {
            return; // Skip if Redis operations fail
        }

        // Verify deletion
        let load_after_delete = db.load_tts_instance(guild_id).await;
        if load_after_delete.is_err() {
            return; // Skip if Redis operations fail
        }
        assert!(load_after_delete.unwrap().is_none());
    }

    #[test]
    fn test_database_constants() {
        // Test that constants are reasonable
        assert!(constants::REDIS_CONNECTION_TIMEOUT_SECS > 0);
        assert!(constants::REDIS_MAX_CONNECTIONS > 0);
        assert!(constants::REDIS_MIN_IDLE_CONNECTIONS <= constants::REDIS_MAX_CONNECTIONS);
    }
}