//! Participant-separated Discord transcription and the optional live Web UI.
pub mod bot;
pub mod bridge;
pub mod protocol;
pub mod router;
#[cfg(feature = "web-ui")]
pub mod web;

pub use bot::Transcription;
use std::sync::Arc;
use tokio::sync::broadcast;

impl Transcription {
    pub fn new(
        config: &crate::config::Config,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> (Arc<Self>, broadcast::Receiver<protocol::ServerMessage>) {
        let (bridge, events) = bridge::BridgeHandle::spawn_with_shutdown(
            config.transcription.url.clone(),
            config.transcription.secret.clone(),
            shutdown,
        );
        let router = Arc::new(router::VoiceRouter::new(
            bridge.clone(),
            config.transcription.require_consent,
        ));
        (
            Arc::new(Self {
                router,
                bridge,
                web_base_url: config.web.enabled.then(|| config.web.base_url.clone()),
                receivers: Default::default(),
            }),
            events,
        )
    }
}

pub fn voice_config(receive: bool) -> songbird::Config {
    use songbird::driver::{Channels, DecodeConfig, DecodeMode, SampleRate};
    songbird::Config::default().decode_mode(if receive {
        DecodeMode::Decode(DecodeConfig::new(Channels::Mono, SampleRate::Hz16000))
    } else {
        DecodeMode::Pass
    })
}

/// Discord permits one connection per bot and guild. Both features use the
/// existing setup lock and must agree on the voice channel before joining.
pub fn ensure_channel(
    data: &crate::data::UserData,
    guild: serenity::all::GuildId,
    channel: serenity::all::ChannelId,
) -> crate::Result<()> {
    if data
        .transcription
        .as_ref()
        .and_then(|t| t.voice_channel(guild))
        .is_some_and(|active| active != channel.get())
    {
        return Err(crate::NCBError::invalid_input(
            "文字起こしが使用中のボイスチャンネルでTTSを開始してください",
        ));
    }
    Ok(())
}
