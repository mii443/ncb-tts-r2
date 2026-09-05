use bb8_redis::{
    bb8::{Pool, PooledConnection},
    redis::{self, AsyncCommands},
    RedisConnectionManager,
};
use serde::{de::DeserializeOwned, Serialize};
use serenity::model::id::{GuildId, UserId};
use std::{collections::HashMap, fmt, future::Future, time::Duration};

use super::{server_config::ServerConfig, user_config::UserConfig};
use crate::{
    errors::{constants::*, NCBError, Result},
    tts::instance::TTSInstance,
};

#[derive(Clone)]
pub struct Database {
    pub pool: Pool<RedisConnectionManager>,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

async fn query<T>(future: impl Future<Output = redis::RedisResult<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(REDIS_CONNECTION_TIMEOUT_SECS), future)
        .await
        .map_err(|_| NCBError::Timeout {
            operation: "Redis command",
        })?
        .map_err(NCBError::from)
}

fn decode<T: DeserializeOwned>(key: &str, raw: &str) -> Result<T> {
    serde_json::from_str(raw)
        .map_err(|_| NCBError::database(format!("Invalid stored configuration at {key}")))
}

impl Database {
    pub fn new(pool: Pool<RedisConnectionManager>) -> Self {
        Self { pool }
    }

    pub async fn new_with_url(redis_url: String) -> Result<Self> {
        let manager = RedisConnectionManager::new(redis_url)?;
        let pool = Pool::builder()
            .max_size(REDIS_MAX_CONNECTIONS)
            .connection_timeout(Duration::from_secs(REDIS_CONNECTION_TIMEOUT_SECS))
            .build(manager)
            .await
            .map_err(|_| NCBError::database("Cannot connect to Redis"))?;
        Ok(Self { pool })
    }

    async fn connection(&self) -> Result<PooledConnection<'_, RedisConnectionManager>> {
        self.pool
            .get()
            .await
            .map_err(|_| NCBError::database("Cannot acquire Redis connection"))
    }

    fn server_key(id: u64) -> String {
        format!("{DISCORD_SERVER_PREFIX}{id}")
    }
    fn user_key(id: u64) -> String {
        format!("{DISCORD_USER_PREFIX}{id}")
    }
    fn tts_instance_key(id: u64) -> String {
        format!("{TTS_INSTANCE_PREFIX}{id}")
    }
    fn tts_instances_list_key() -> String {
        TTS_INSTANCES_LIST_KEY.into()
    }
    fn user_config_key(guild: u64, user: u64) -> String {
        format!("user:config:{guild}:{user}")
    }
    fn server_config_key(guild: u64) -> String {
        format!("server:config:{guild}")
    }
    fn dictionary_key(guild: u64) -> String {
        format!("dictionary:{guild}")
    }

    #[tracing::instrument(skip_all)]
    async fn get_config<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let mut connection = self.connection().await?;
        let raw: Option<String> = query(connection.get(key)).await?;
        raw.as_deref().map(|raw| decode(key, raw)).transpose()
    }

    #[tracing::instrument(skip_all)]
    async fn set_config<T: Serialize>(&self, key: &str, config: &T) -> Result<()> {
        let raw = serde_json::to_string(config)?;
        let mut connection = self.connection().await?;
        query(connection.set::<_, _, ()>(key, raw)).await
    }

    async fn set_if_missing<T: Serialize>(&self, key: &str, config: &T) -> Result<()> {
        let raw = serde_json::to_string(config)?;
        let mut connection = self.connection().await?;
        let _: bool = query(connection.set_nx(key, raw)).await?;
        Ok(())
    }

    async fn delete_key(&self, key: &str) -> Result<()> {
        let mut connection = self.connection().await?;
        query(connection.del::<_, ()>(key)).await
    }

    /// Compare-and-set does not leave WATCH state on a pooled connection when cancelled.
    async fn update_config<T, F>(&self, key: &str, default: T, update: F) -> Result<T>
    where
        T: Serialize + DeserializeOwned + Clone,
        F: Fn(&mut T) -> Result<()>,
    {
        let script =
            "local current = redis.call('GET', KEYS[1])
             if (ARGV[1] == '0' and current ~= false) or (ARGV[1] == '1' and current ~= ARGV[2]) then return 0 end
             redis.call('SET', KEYS[1], ARGV[3])
             return 1";
        let mut connection = self.connection().await?;
        for _ in 0..16 {
            let old: Option<String> = query(connection.get(key)).await?;
            let mut config: T = match &old {
                Some(raw) => decode(key, raw)?,
                None => default.clone(),
            };
            update(&mut config)?;
            let new = serde_json::to_string(&config)?;
            let applied: bool = query(
                redis::cmd("EVAL")
                    .arg(script)
                    .arg(1)
                    .arg(key)
                    .arg(if old.is_some() { "1" } else { "0" })
                    .arg(old.as_deref().unwrap_or(""))
                    .arg(new)
                    .query_async(&mut *connection),
            )
            .await?;
            if applied {
                return Ok(config);
            }
            tokio::task::yield_now().await;
        }
        Err(NCBError::database(
            "Configuration changed concurrently; please retry",
        ))
    }

    pub async fn get_server_config(&self, id: u64) -> Result<Option<ServerConfig>> {
        self.get_config(&Self::server_key(id)).await
    }
    pub async fn get_user_config(&self, id: u64) -> Result<Option<UserConfig>> {
        self.get_config(&Self::user_key(id)).await
    }
    pub async fn set_server_config(&self, id: u64, config: ServerConfig) -> Result<()> {
        self.set_config(&Self::server_key(id), &config).await
    }
    pub async fn set_user_config(&self, id: u64, config: UserConfig) -> Result<()> {
        self.set_config(&Self::user_key(id), &config).await
    }
    pub async fn set_default_server_config(&self, id: u64) -> Result<()> {
        self.set_if_missing(&Self::server_key(id), &ServerConfig::default())
            .await
    }
    pub async fn set_default_user_config(&self, id: u64) -> Result<()> {
        self.set_if_missing(&Self::user_key(id), &UserConfig::default())
            .await
    }

    pub async fn get_server_config_or_default(&self, id: u64) -> Result<Option<ServerConfig>> {
        match self.get_server_config(id).await? {
            Some(config) => Ok(Some(config)),
            None => {
                self.set_default_server_config(id).await?;
                self.get_server_config(id).await
            }
        }
    }

    pub async fn get_user_config_or_default(&self, id: u64) -> Result<Option<UserConfig>> {
        match self.get_user_config(id).await? {
            Some(config) => Ok(Some(config)),
            None => {
                self.set_default_user_config(id).await?;
                self.get_user_config(id).await
            }
        }
    }

    pub async fn update_server_config<F>(&self, id: u64, update: F) -> Result<ServerConfig>
    where
        F: Fn(&mut ServerConfig) -> Result<()>,
    {
        self.update_config(&Self::server_key(id), ServerConfig::default(), update)
            .await
    }

    pub async fn update_user_config<F>(&self, id: u64, update: F) -> Result<UserConfig>
    where
        F: Fn(&mut UserConfig) -> Result<()>,
    {
        self.update_config(&Self::user_key(id), UserConfig::default(), update)
            .await
    }

    pub async fn save_tts_instance(&self, guild: GuildId, instance: &TTSInstance) -> Result<()> {
        let raw = serde_json::to_string(instance)?;
        let mut connection = self.connection().await?;
        query(
            redis::cmd("EVAL")
                .arg("local kind = redis.call('TYPE', KEYS[2]).ok
                      if kind ~= 'none' and kind ~= 'set' then return redis.error_reply('Invalid session index type') end
                      redis.call('SET', KEYS[1], ARGV[1])
                      redis.call('SADD', KEYS[2], ARGV[2])
                      return 1")
                .arg(2)
                .arg(Self::tts_instance_key(guild.get()))
                .arg(Self::tts_instances_list_key())
                .arg(raw)
                .arg(guild.get())
                .query_async::<()>(&mut *connection),
        )
        .await
    }

    pub async fn load_tts_instance(&self, guild: GuildId) -> Result<Option<TTSInstance>> {
        let instance: Option<TTSInstance> = self
            .get_config(&Self::tts_instance_key(guild.get()))
            .await?;
        if instance
            .as_ref()
            .is_some_and(|instance| instance.guild != guild || instance.text_channels.is_empty())
        {
            return Err(NCBError::database("Invalid saved TTS instance"));
        }
        Ok(instance)
    }

    pub async fn remove_tts_instance(&self, guild: GuildId) -> Result<()> {
        let mut connection = self.connection().await?;
        query(
            redis::cmd("EVAL")
                .arg("local kind = redis.call('TYPE', KEYS[2]).ok
                      if kind ~= 'none' and kind ~= 'set' then return redis.error_reply('Invalid session index type') end
                      redis.call('DEL', KEYS[1])
                      redis.call('SREM', KEYS[2], ARGV[1])
                      return 1")
                .arg(2)
                .arg(Self::tts_instance_key(guild.get()))
                .arg(Self::tts_instances_list_key())
                .arg(guild.get())
                .query_async::<()>(&mut *connection),
        )
        .await
    }

    pub async fn get_all_tts_instances(&self) -> Result<Vec<(GuildId, TTSInstance)>> {
        let ids = self.list_active_instances().await?;
        let mut instances = Vec::new();
        for id in ids {
            let guild = GuildId::new(id);
            if let Some(instance) = self.load_tts_instance(guild).await? {
                instances.push((guild, instance));
            }
        }
        Ok(instances)
    }

    pub async fn save_user_config(
        &self,
        guild: GuildId,
        user: UserId,
        config: &UserConfig,
    ) -> Result<()> {
        self.set_config(&Self::user_config_key(guild.get(), user.get()), config)
            .await
    }
    pub async fn load_user_config(
        &self,
        guild: GuildId,
        user: UserId,
    ) -> Result<Option<UserConfig>> {
        self.get_config(&Self::user_config_key(guild.get(), user.get()))
            .await
    }
    pub async fn delete_user_config(&self, guild: GuildId, user: UserId) -> Result<()> {
        self.delete_key(&Self::user_config_key(guild.get(), user.get()))
            .await
    }
    pub async fn save_server_config(&self, guild: GuildId, config: &ServerConfig) -> Result<()> {
        self.set_config(&Self::server_config_key(guild.get()), config)
            .await
    }
    pub async fn load_server_config(&self, guild: GuildId) -> Result<Option<ServerConfig>> {
        self.get_config(&Self::server_config_key(guild.get())).await
    }
    pub async fn delete_server_config(&self, guild: GuildId) -> Result<()> {
        self.delete_key(&Self::server_config_key(guild.get())).await
    }
    pub async fn save_dictionary(
        &self,
        guild: GuildId,
        dictionary: &HashMap<String, String>,
    ) -> Result<()> {
        self.set_config(&Self::dictionary_key(guild.get()), dictionary)
            .await
    }
    pub async fn load_dictionary(&self, guild: GuildId) -> Result<HashMap<String, String>> {
        Ok(self
            .get_config(&Self::dictionary_key(guild.get()))
            .await?
            .unwrap_or_default())
    }
    pub async fn delete_dictionary(&self, guild: GuildId) -> Result<()> {
        self.delete_key(&Self::dictionary_key(guild.get())).await
    }
    pub async fn delete_tts_instance(&self, guild: GuildId) -> Result<()> {
        self.remove_tts_instance(guild).await
    }
    pub async fn list_active_instances(&self) -> Result<Vec<u64>> {
        let mut connection = self.connection().await?;
        query(connection.smembers(Self::tts_instances_list_key())).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::dictionary::Rule;
    use serenity::model::id::ChannelId;
    use std::process::{Child, Command, Stdio};
    use tempfile::TempDir;

    struct RedisTest {
        child: Child,
        _directory: TempDir,
    }
    impl Drop for RedisTest {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    async fn isolated_database() -> (RedisTest, Database) {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("redis.sock");
        let child = Command::new("redis-server")
            .args([
                "--port",
                "0",
                "--save",
                "",
                "--appendonly",
                "no",
                "--unixsocket",
            ])
            .arg(&socket)
            .arg("--dir")
            .arg(directory.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("redis-server is required");
        let guard = RedisTest {
            child,
            _directory: directory,
        };
        for _ in 0..100 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let db = Database::new_with_url(format!("redis+unix://{}", socket.display()))
            .await
            .unwrap();
        (guard, db)
    }

    #[test]
    fn invalid_and_empty_json_are_errors_not_missing_settings() {
        assert!(decode::<ServerConfig>("test", "invalid").is_err());
        assert!(decode::<ServerConfig>("test", "").is_err());
        assert!(decode::<ServerConfig>("test", "{}").is_err());
    }

    #[tokio::test]
    #[ignore = "requires redis-server; starts an isolated Unix socket server"]
    async fn redis_errors_and_corrupt_settings_do_not_create_defaults() {
        let (_server, db) = isolated_database().await;
        let key = Database::server_key(123);
        let mut connection = db.connection().await.unwrap();
        let _: () = connection.set(&key, "broken-json").await.unwrap();
        assert!(db.get_server_config_or_default(123).await.is_err());
        assert_eq!(
            connection.get::<_, String>(&key).await.unwrap(),
            "broken-json"
        );
        let _: () = connection.del(&key).await.unwrap();
        let _: () = connection.lpush(&key, "wrong-type").await.unwrap();
        assert!(db.get_server_config_or_default(123).await.is_err());
        assert_eq!(connection.llen::<_, usize>(&key).await.unwrap(), 1);
        let _: () = connection.del(&key).await.unwrap();
        let config = ServerConfig {
            autostart_channel_id: Some(999),
            ..ServerConfig::default()
        };
        db.set_server_config(123, config.clone()).await.unwrap();
        db.set_default_server_config(123).await.unwrap();
        assert_eq!(db.get_server_config(123).await.unwrap(), Some(config));
    }

    #[tokio::test]
    #[ignore = "requires redis-server; starts an isolated Unix socket server"]
    async fn concurrent_dictionary_updates_do_not_overwrite_each_other() {
        let (_server, db) = isolated_database().await;
        let mut tasks = tokio::task::JoinSet::new();
        for i in 0..8 {
            let db = db.clone();
            tasks.spawn(async move {
                db.update_server_config(123, |config| {
                    config.dictionary.rules.push(Rule {
                        id: i.to_string(),
                        rule: i.to_string(),
                        to: i.to_string(),
                        is_regex: false,
                    });
                    Ok(())
                })
                .await
                .unwrap();
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
        let config = db.get_server_config(123).await.unwrap().unwrap();
        for i in 0..8 {
            assert_eq!(
                config
                    .dictionary
                    .rules
                    .iter()
                    .filter(|rule| rule.id == i.to_string())
                    .count(),
                1
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires redis-server; starts an isolated Unix socket server"]
    async fn tts_instance_roundtrip_and_command_errors() {
        let (_server, db) = isolated_database().await;
        let guild = GuildId::new(123);
        let instance = TTSInstance::new_single(ChannelId::new(1), ChannelId::new(2), guild);
        db.save_tts_instance(guild, &instance).await.unwrap();
        let restored = db.get_all_tts_instances().await.unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].1.voice_channel, instance.voice_channel);
        db.remove_tts_instance(guild).await.unwrap();
        assert!(db.get_all_tts_instances().await.unwrap().is_empty());
        assert!(db.load_tts_instance(guild).await.unwrap().is_none());
        let mut connection = db.connection().await.unwrap();
        let _: () = connection
            .set(Database::tts_instances_list_key(), "wrong-type")
            .await
            .unwrap();
        assert!(db.list_active_instances().await.is_err());
        let saved_key = Database::tts_instance_key(guild.get());
        let _: () = connection
            .set(&saved_key, serde_json::to_string(&instance).unwrap())
            .await
            .unwrap();
        assert!(db.remove_tts_instance(guild).await.is_err());
        assert!(db.load_tts_instance(guild).await.unwrap().is_some());
        assert!(db.save_tts_instance(guild, &instance).await.is_err());
        assert_eq!(
            connection
                .get::<_, String>(Database::tts_instances_list_key())
                .await
                .unwrap(),
            "wrong-type"
        );
    }
}
