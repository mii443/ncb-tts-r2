use serde::{Deserialize, Serialize};
use serenity::{
    model::{
        channel::Message,
        id::{ChannelId, GuildId},
    },
    prelude::Context,
};

use crate::data::UserData;

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
        let manager = &ctx.data::<UserData>().songbird;
        let Some(call) = manager.get(self.guild) else {
            return false;
        };
        let call = call.lock().await;
        call.current_connection()
            .and_then(|connection| connection.channel_id)
            .is_some_and(|id| id.get() == self.voice_channel.get())
    }

    #[tracing::instrument(skip_all)]
    pub async fn reconnect(&self, ctx: &Context, _skip_check: bool) -> crate::errors::Result<()> {
        let data = ctx.data::<UserData>();
        #[cfg(feature = "transcription")]
        if let Some(transcription) = &data.transcription {
            let call = data.songbird.get_or_insert(self.guild);
            transcription
                .install_receiver(ctx, self.guild, self.voice_channel, &call)
                .await;
        }
        // Gateway connection info can survive a driver failure. Songbird's join
        // also checks the driver and is a no-op when it is already connected.
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            data.songbird.join(self.guild, self.voice_channel),
        )
        .await
        .map_err(|_| crate::errors::NCBError::Timeout {
            operation: "Voice connection",
        })?
        .map_err(|_| crate::errors::NCBError::voice_connection("Failed to join voice channel"))?;
        Ok(())
    }
}
