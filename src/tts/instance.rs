use serenity::{
    model::{
        channel::Message,
        id::{ChannelId, GuildId},
    },
    prelude::Context,
};

use crate::tts::message::TTSMessage;

pub struct TTSInstance {
    pub before_message: Option<Message>,
    pub text_channel: ChannelId,
    pub voice_channel: ChannelId,
    pub guild: GuildId,
}

impl TTSInstance {
    /// Synthesize text to speech and send it to the voice channel.
    ///
    /// Example:
    /// ```rust
    /// instance.read(message, &ctx).await;
    /// ```
    pub async fn read<T>(&mut self, message: T, ctx: &Context)
    where
        T: TTSMessage,
    {
        let path = message.synthesize(self, ctx).await;

        {
            let manager = songbird::get(&ctx).await.unwrap();
            let call = manager.get(self.guild).unwrap();
            let mut call = call.lock().await;
            let input = songbird::input::ffmpeg(path)
                .await
                .expect("File not found.");
            call.enqueue_source(input);
        }
    }

    pub async fn skip(&mut self, ctx: &Context) {
        let manager = songbird::get(&ctx).await.unwrap();
        let call = manager.get(self.guild).unwrap();
        let call = call.lock().await;
        let queue = call.queue();
        let _ = queue.skip();
    }
}
