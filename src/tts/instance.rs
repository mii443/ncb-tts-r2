use std::fmt::Debug;

use serde::{Deserialize, Serialize};
use serenity::{
    model::{
        channel::Message,
        id::{ChannelId, GuildId},
    },
    prelude::Context,
};

use crate::data::UserData;
use crate::tts::message::TTSMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSInstance {
    #[serde(skip)]
    pub before_message: Option<Message>,
    pub text_channels: Vec<ChannelId>,
    pub voice_channel: ChannelId,
    pub guild: GuildId,
}

impl TTSInstance {
    pub fn new(text_channels: Vec<ChannelId>, voice_channel: ChannelId, guild: GuildId) -> Self {
        Self {
            before_message: None,
            text_channels,
            voice_channel,
            guild,
        }
    }

    pub fn new_single(text_channel: ChannelId, voice_channel: ChannelId, guild: GuildId) -> Self {
        Self::new(vec![text_channel], voice_channel, guild)
    }

    pub fn add_text_channel(&mut self, channel_id: ChannelId) {
        if !self.text_channels.contains(&channel_id) {
            self.text_channels.push(channel_id);
        }
    }

    pub fn remove_text_channel(&mut self, channel_id: ChannelId) -> bool {
        if let Some(pos) = self.text_channels.iter().position(|&x| x == channel_id) {
            self.text_channels.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn contains_text_channel(&self, channel_id: ChannelId) -> bool {
        self.text_channels.contains(&channel_id)
    }

    pub fn get_text_channels(&self) -> &Vec<ChannelId> {
        &self.text_channels
    }

    pub async fn check_connection(&self, ctx: &Context) -> bool {
        let manager = ctx.data::<UserData>().songbird.clone();

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

    #[tracing::instrument(skip_all)]
    pub async fn reconnect(
        &self,
        ctx: &Context,
        skip_check: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let manager = ctx.data::<UserData>().songbird.clone();

        if self.check_connection(&ctx).await {
            tracing::info!("Already connected to guild {}", self.guild);
            return Ok(());
        }

        match manager.join(self.guild, self.voice_channel).await {
            Ok(_) => {
                tracing::info!(
                    "Successfully reconnected to voice channel {} in guild {}",
                    self.voice_channel,
                    self.guild
                );

                match self.guild.channels(&ctx.http).await {
                    Ok(channels) => {
                        if let Some(channel) = channels.get(&self.voice_channel) {
                            match channel.members(&ctx.cache) {
                                Ok(members) => {
                                    let user_count =
                                        members.iter().filter(|member| !member.user.bot()).count();
                                    if user_count == 0 {
                                        tracing::info!("No users found in voice channel after reconnection, disconnecting from guild {}", self.guild);
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

    #[tracing::instrument(skip_all)]
    pub async fn read<T>(&mut self, message: T, ctx: &Context)
    where
        T: TTSMessage + Debug,
    {
        let audio = message.synthesize(self, ctx).await;

        {
            let manager = ctx.data::<UserData>().songbird.clone();
            let call = manager.get(self.guild).unwrap();
            let mut call = call.lock().await;
            for audio in audio {
                call.enqueue(audio.into()).await;
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn skip(&mut self, ctx: &Context) {
        let manager = ctx.data::<UserData>().songbird.clone();
        let call = manager.get(self.guild).unwrap();
        let call = call.lock().await;
        let queue = call.queue();
        let _ = queue.skip();
    }
}
