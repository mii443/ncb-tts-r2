use serenity::{model::channel::Message, prelude::Context, all::{CreateMessage, CreateEmbed}};
use std::time::Duration;
use tokio::time;
use tracing::{error, info, warn};

use crate::data::{DatabaseClientData, TTSData};

/// Connection monitor that periodically checks voice channel connections
pub struct ConnectionMonitor;

impl ConnectionMonitor {
    /// Start the connection monitoring task
    pub fn start(ctx: Context) {
        tokio::spawn(async move {
            info!("Starting connection monitor with 5s interval");
            let mut interval = time::interval(Duration::from_secs(5));

            loop {
                interval.tick().await;
                Self::check_connections(&ctx).await;
            }
        });
    }

    /// Check all active TTS instances and their voice channel connections
    async fn check_connections(ctx: &Context) {
        let storage_lock = {
            let data_read = ctx.data.read().await;
            data_read
                .get::<TTSData>()
                .expect("Cannot get TTSStorage")
                .clone()
        };

        let database = {
            let data_read = ctx.data.read().await;
            data_read
                .get::<DatabaseClientData>()
                .expect("Cannot get DatabaseClientData")
                .clone()
        };

        let mut storage = storage_lock.write().await;
        let mut guilds_to_remove = Vec::new();

        for (guild_id, instance) in storage.iter() {
            // Check if bot is still connected to voice channel
            let manager = match songbird::get(ctx).await {
                Some(manager) => manager,
                None => {
                    error!("Cannot get songbird manager");
                    continue;
                }
            };

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
                warn!("Bot disconnected from voice channel in guild {}", guild_id);

                // Check if there are users in the voice channel
                let should_reconnect = match Self::check_voice_channel_users(ctx, instance).await {
                    Ok(has_users) => has_users,
                    Err(_) => {
                        // If we can't check users, don't reconnect
                        false
                    }
                };

                if should_reconnect {
                    // Try to reconnect
                    match instance.reconnect(ctx, true).await {
                        Ok(_) => {
                            info!(
                                "Successfully reconnected to voice channel in guild {}",
                                guild_id
                            );
                            
                            // Send notification message to text channel with embed
                            let embed = CreateEmbed::new()
                                .title("🔄 自動再接続しました")
                                .description("読み上げを停止したい場合は `/stop` コマンドを使用してください。")
                                .color(0x00ff00);
                            if let Err(e) = instance.text_channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await {
                                error!("Failed to send reconnection message to text channel: {}", e);
                            }
                        }
                        Err(e) => {
                            error!(
                                "Failed to reconnect to voice channel in guild {}: {}",
                                guild_id, e
                            );
                            guilds_to_remove.push(*guild_id);
                        }
                    }
                } else {
                    info!(
                        "No users in voice channel, removing instance for guild {}",
                        guild_id
                    );
                    guilds_to_remove.push(*guild_id);
                }
            }
        }

        // Remove disconnected instances
        for guild_id in guilds_to_remove {
            storage.remove(&guild_id);

            // Remove from database
            if let Err(e) = database.remove_tts_instance(guild_id).await {
                error!("Failed to remove TTS instance from database: {}", e);
            }

            // Ensure bot leaves voice channel
            if let Some(manager) = songbird::get(ctx).await {
                let _ = manager.remove(guild_id).await;
            }
        }
    }

    /// Check if there are users in the voice channel
    async fn check_voice_channel_users(
        ctx: &Context,
        instance: &crate::tts::instance::TTSInstance,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let channels = instance.guild.channels(&ctx.http).await?;

        if let Some(channel) = channels.get(&instance.voice_channel) {
            let members = channel.members(&ctx.cache)?;
            let user_count = members.iter().filter(|member| !member.user.bot).count();
            Ok(user_count > 0)
        } else {
            // Channel doesn't exist anymore
            Ok(false)
        }
    }
}
