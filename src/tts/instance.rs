use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use serenity::{
    model::{
        channel::Message,
        id::{ChannelId, GuildId},
    },
    prelude::Context,
};

use crate::tts::message::TTSMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSInstance {
    #[serde(skip)] // Messageは複雑すぎるのでシリアライズしない
    pub before_message: Option<Message>,
    pub text_channels: Vec<ChannelId>,
    pub voice_channel: ChannelId,
    pub guild: GuildId,
}

impl TTSInstance {
    /// Create a new TTSInstance
    pub fn new(text_channels: Vec<ChannelId>, voice_channel: ChannelId, guild: GuildId) -> Self {
        Self {
            before_message: None,
            text_channels,
            voice_channel,
            guild,
        }
    }

    /// Create a new TTSInstance with a single text channel
    pub fn new_single(text_channel: ChannelId, voice_channel: ChannelId, guild: GuildId) -> Self {
        Self::new(vec![text_channel], voice_channel, guild)
    }

    /// Add a text channel to the instance
    pub fn add_text_channel(&mut self, channel_id: ChannelId) {
        if !self.text_channels.contains(&channel_id) {
            self.text_channels.push(channel_id);
        }
    }

    /// Remove a text channel from the instance
    pub fn remove_text_channel(&mut self, channel_id: ChannelId) -> bool {
        if let Some(pos) = self.text_channels.iter().position(|&x| x == channel_id) {
            self.text_channels.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check if a channel is in the text channels list
    pub fn contains_text_channel(&self, channel_id: ChannelId) -> bool {
        self.text_channels.contains(&channel_id)
    }

    /// Get all text channels
    pub fn get_text_channels(&self) -> &Vec<ChannelId> {
        &self.text_channels
    }

    pub async fn check_connection(&self, ctx: &Context) -> bool {
        let manager = match songbird::get(ctx).await {
            Some(manager) => manager,
            None => {
                tracing::error!("Cannot get songbird manager");
                return false;
            }
        };

        let call = manager.get(self.guild);
        if let Some(call) = call {
            if let Some(connection) = call.lock().await.current_connection() {
                connection.channel_id.is_some()
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Reconnect to the voice channel after bot restart
    #[tracing::instrument]
    pub async fn reconnect(
        &self,
        ctx: &Context,
        skip_check: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = songbird::get(&ctx)
            .await
            .ok_or("Songbird manager not available")?;

        // Check if we're already connected
        if self.check_connection(&ctx).await {
            tracing::info!("Already connected to guild {}", self.guild);
            return Ok(());
        }

        // Try to connect to the voice channel
        match manager.join(self.guild, self.voice_channel).await {
            Ok(_) => {
                tracing::info!(
                    "Successfully reconnected to voice channel {} in guild {}",
                    self.voice_channel,
                    self.guild
                );

                // Double-check if there are users in the voice channel after connection
                match self.guild.channels(&ctx.http).await {
                    Ok(channels) => {
                        if let Some(channel) = channels.get(&self.voice_channel) {
                            match channel.members(&ctx.cache) {
                                Ok(members) => {
                                    let user_count =
                                        members.iter().filter(|member| !member.user.bot).count();
                                    if user_count == 0 {
                                        tracing::info!("No users found in voice channel after reconnection, disconnecting from guild {}", self.guild);
                                        // Disconnect if no users are present
                                        let _ = manager.remove(self.guild).await;
                                        return Err(
                                            "No users in voice channel after reconnection".into()
                                        );
                                    }
                                }
                                Err(_) => {
                                    tracing::warn!(
                                        "Failed to verify members after reconnection for guild {}",
                                        self.guild
                                    );
                                }
                            }
                        }
                    }
                    Err(_) => {
                        tracing::warn!(
                            "Failed to get channels after reconnection for guild {}",
                            self.guild
                        );
                    }
                }

                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to reconnect to voice channel: {}", e);
                Err(Box::new(e))
            }
        }
    }

    /// Synthesize text to speech and send it to the voice channel.
    ///
    /// Example:
    /// ```rust
    /// instance.read(message, &ctx).await;
    /// ```
    #[tracing::instrument]
    pub async fn read<T>(&mut self, message: T, ctx: &Context)
    where
        T: TTSMessage + Debug,
    {
        let audio = message.synthesize(self, ctx).await;

        {
            let manager = songbird::get(&ctx).await.unwrap();
            let call = manager.get(self.guild).unwrap();
            let mut call = call.lock().await;
            for audio in audio {
                call.enqueue(audio.into()).await;
            }
        }
    }

    #[tracing::instrument]
    pub async fn skip(&mut self, ctx: &Context) {
        let manager = songbird::get(&ctx).await.unwrap();
        let call = manager.get(self.guild).unwrap();
        let call = call.lock().await;
        let queue = call.queue();
        let _ = queue.skip();
    }
}
