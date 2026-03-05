use serenity::{
    all::{CreateEmbed, CreateMessage},
    prelude::Context,
};
use std::time::Duration;
use tokio::time;
use tracing::{error, info, instrument, warn};

use crate::data::UserData;

const CONNECTION_CHECK_INTERVAL_SECS: u64 = 5;
const MAX_RECONNECTION_ATTEMPTS: u32 = 3;
const RECONNECTION_BACKOFF_SECS: u64 = 2;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionMonitorError {
    #[error("Failed to get songbird manager")]
    SongbirdManagerNotFound,
    #[error("Failed to check voice channel users: {0}")]
    VoiceChannelCheck(String),
    #[error("Failed to reconnect after {attempts} attempts")]
    ReconnectionFailed { attempts: u32 },
    #[error("Database operation failed: {0}")]
    Database(#[from] redis::RedisError),
}

type Result<T> = std::result::Result<T, ConnectionMonitorError>;

pub struct ConnectionMonitor {
    reconnection_attempts: std::collections::HashMap<serenity::model::id::GuildId, u32>,
}

impl Default for ConnectionMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionMonitor {
    pub fn new() -> Self {
        Self {
            reconnection_attempts: std::collections::HashMap::new(),
        }
    }

    pub fn start(ctx: Context) {
        tokio::spawn(async move {
            let mut monitor = ConnectionMonitor::new();
            info!(
                interval_secs = CONNECTION_CHECK_INTERVAL_SECS,
                "Starting connection monitor"
            );
            let mut interval = time::interval(Duration::from_secs(CONNECTION_CHECK_INTERVAL_SECS));

            loop {
                interval.tick().await;
                if let Err(e) = monitor.check_connections(&ctx).await {
                    error!(error = %e, "Connection monitoring failed");
                }
            }
        });
    }

    #[instrument(skip(self, ctx))]
    async fn check_connections(&mut self, ctx: &Context) -> Result<()> {
        let data = ctx.data::<UserData>();
        let storage_lock = data.tts_data.clone();
        let database = data.database.clone();
        let manager = data.songbird.clone();

        let mut storage = storage_lock.write().await;
        let mut guilds_to_remove = Vec::new();

        for (guild_id, instance) in storage.iter() {
            let call = manager.get(*guild_id);
            let is_connected = if let Some(call) = call {
                if let Some(connection) = call.lock().await.current_connection() {
                    connection.channel_id.is_some()
                } else {
                    false
                }
            } else {
                false
            };

            if !is_connected {
                warn!(guild_id = %guild_id, "Bot disconnected from voice channel");

                let should_reconnect = match self.check_voice_channel_users(ctx, instance).await {
                    Ok(has_users) => has_users,
                    Err(e) => {
                        warn!(guild_id = %guild_id, error = %e, "Failed to check voice channel users, skipping reconnection");
                        false
                    }
                };

                if should_reconnect {
                    let attempts = self
                        .reconnection_attempts
                        .get(guild_id)
                        .copied()
                        .unwrap_or(0);

                    if attempts >= MAX_RECONNECTION_ATTEMPTS {
                        error!(
                            guild_id = %guild_id,
                            attempts = attempts,
                            "Maximum reconnection attempts reached, removing instance"
                        );
                        guilds_to_remove.push(*guild_id);
                        self.reconnection_attempts.remove(guild_id);
                        continue;
                    }

                    if attempts > 0 {
                        let backoff_duration =
                            Duration::from_secs(RECONNECTION_BACKOFF_SECS * (2_u64.pow(attempts)));
                        warn!(
                            guild_id = %guild_id,
                            attempt = attempts + 1,
                            backoff_secs = backoff_duration.as_secs(),
                            "Applying backoff before reconnection attempt"
                        );
                        tokio::time::sleep(backoff_duration).await;
                    }

                    match instance.reconnect(ctx, true).await {
                        Ok(_) => {
                            info!(
                                guild_id = %guild_id,
                                attempts = attempts + 1,
                                "Successfully reconnected to voice channel"
                            );

                            self.reconnection_attempts.remove(guild_id);

                            let embed = CreateEmbed::new()
                                .title("🔄 自動再接続しました")
                                .description("読み上げを停止したい場合は `/stop` コマンドを使用してください。")
                                .color(0x00ff00);

                            if let Some(&text_channel) = instance.text_channels.first() {
                                let msg = CreateMessage::new().embed(embed);
                                if let Err(e) = text_channel
                                    .widen().send_message(&ctx.http, msg)
                                    .await
                                {
                                    error!(guild_id = %guild_id, error = %e, "Failed to send reconnection message");
                                }
                            }
                        }
                        Err(e) => {
                            let new_attempts = attempts + 1;
                            self.reconnection_attempts.insert(*guild_id, new_attempts);
                            error!(
                                guild_id = %guild_id,
                                attempt = new_attempts,
                                error = %e,
                                "Failed to reconnect to voice channel"
                            );

                            if new_attempts >= MAX_RECONNECTION_ATTEMPTS {
                                guilds_to_remove.push(*guild_id);
                                self.reconnection_attempts.remove(guild_id);
                            }
                        }
                    }
                } else {
                    info!(
                        guild_id = %guild_id,
                        "No users in voice channel, removing instance"
                    );
                    guilds_to_remove.push(*guild_id);
                    self.reconnection_attempts.remove(guild_id);
                }
            }
        }

        for guild_id in guilds_to_remove {
            storage.remove(&guild_id);

            if let Err(e) = database.remove_tts_instance(guild_id).await {
                error!(guild_id = %guild_id, error = %e, "Failed to remove TTS instance from database");
            }

            if let Err(e) = manager.remove(guild_id).await {
                error!(guild_id = %guild_id, error = %e, "Failed to remove bot from voice channel");
            }

            info!(guild_id = %guild_id, "Removed disconnected TTS instance");
        }

        Ok(())
    }

    #[instrument(skip(self, ctx, instance))]
    async fn check_voice_channel_users(
        &self,
        ctx: &Context,
        instance: &crate::tts::instance::TTSInstance,
    ) -> Result<bool> {
        let channels = instance.guild.channels(&ctx.http).await.map_err(|e| {
            ConnectionMonitorError::VoiceChannelCheck(format!(
                "Failed to get guild channels: {}",
                e
            ))
        })?;

        if let Some(channel) = channels.get(&instance.voice_channel) {
            let members = channel.members(&ctx.cache).map_err(|e| {
                ConnectionMonitorError::VoiceChannelCheck(format!(
                    "Failed to get channel members: {}",
                    e
                ))
            })?;
            let user_count = members.iter().filter(|member| !member.user.bot()).count();

            info!(
                guild_id = %instance.guild,
                channel_id = %instance.voice_channel,
                user_count = user_count,
                "Checked voice channel users"
            );

            Ok(user_count > 0)
        } else {
            warn!(
                guild_id = %instance.guild,
                channel_id = %instance.voice_channel,
                "Voice channel no longer exists"
            );
            Ok(false)
        }
    }
}
