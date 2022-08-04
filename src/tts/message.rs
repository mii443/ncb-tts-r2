use async_trait::async_trait;
use serenity::prelude::Context;

use crate::tts::instance::TTSInstance;

/// Message trait that can be used to synthesize text to speech.
#[async_trait]
pub trait TTSMessage {

    /// Parse the message for synthesis.
    ///
    /// Example:
    /// ```rust
    /// let text = message.parse(instance, ctx).await;
    /// ```
    async fn parse(&self, instance: &mut TTSInstance, ctx: &Context) -> String;

    /// Synthesize the message and returns the path to the audio file.
    ///
    /// Example:
    /// ```rust
    /// let path = message.synthesize(instance, ctx).await;
    /// ```
    async fn synthesize(&self, instance: &mut TTSInstance, ctx: &Context) -> String;
}
