use async_trait::async_trait;
use serenity::{model::id::GuildId, prelude::Context};
use songbird::tracks::{PlayMode, TrackHandle};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{watch, Mutex};

use super::{
    instance::TTSInstance,
    message::TTSMessage,
    worker::{Processor, WorkQueue},
};
use crate::{
    data::UserData,
    errors::{constants::TTS_TIMEOUT_SECS, NCBError, Result},
};

pub const SPEECH_QUEUE_CAPACITY: usize = 32;

pub struct TTSSession {
    pub instance: TTSInstance,
    queue: WorkQueue<Box<dyn TTSMessage>>,
    connected: watch::Sender<bool>,
    connection_lock: Mutex<()>,
    persisted: AtomicBool,
}

impl TTSSession {
    pub fn new(instance: TTSInstance, ctx: Context, persisted: bool) -> Arc<Self> {
        let (connected, ready) = watch::channel(false);
        let queue = WorkQueue::start(
            SPEECH_QUEUE_CAPACITY,
            SpeechProcessor {
                instance: instance.clone(),
                ctx,
                ready,
            },
        );
        Arc::new(Self {
            instance,
            queue,
            connected,
            connection_lock: Mutex::new(()),
            persisted: AtomicBool::new(persisted),
        })
    }

    pub fn enqueue(&self, message: impl TTSMessage + 'static) -> Result<()> {
        self.queue.enqueue(Box::new(message))
    }
    pub fn skip(&self) {
        self.queue.skip();
    }
    pub fn cancel(&self) {
        self.queue.stop();
        self.connected.send_replace(false);
    }
    pub fn is_stopped(&self) -> bool {
        self.queue.is_stopped()
    }

    pub async fn connect(&self, ctx: &Context) -> Result<()> {
        let _guard = self.connection_lock.lock().await;
        if self.is_stopped() {
            return Err(NCBError::SessionStopped);
        }
        let data = ctx.data::<UserData>();
        let _setup_guard = data.setup_guard(self.instance.guild).await;
        #[cfg(feature = "transcription")]
        crate::transcription::ensure_channel(
            &data,
            self.instance.guild,
            self.instance.voice_channel,
        )?;
        let cancel = self.queue.cancellation();
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(NCBError::SessionStopped),
            result = async {
                if !self.persisted.load(Ordering::Acquire) {
                    ctx.data::<UserData>().database.save_tts_instance(self.instance.guild, &self.instance).await?;
                    self.persisted.store(true, Ordering::Release);
                }
                self.instance.reconnect(ctx, false).await
            } => result,
        };
        self.connected.send_replace(result.is_ok());
        result
    }

    pub async fn stop(self: &Arc<Self>, ctx: &Context) -> Result<()> {
        // Cancel synthesis/playback before waiting for I/O or a reconnect to unwind.
        self.cancel();
        let _guard = self.connection_lock.lock().await;
        let data = ctx.data::<UserData>();
        let _setup_guard = data.setup_guard(self.instance.guild).await;
        // A second cleanup attempt must not disconnect a replacement session.
        if !data
            .tts_data
            .read()
            .await
            .get(&self.instance.guild)
            .is_some_and(|session| Arc::ptr_eq(session, self))
        {
            return Ok(());
        }
        #[cfg(feature = "transcription")]
        let keep_voice = data
            .transcription
            .as_ref()
            .and_then(|t| t.voice_channel(self.instance.guild))
            .is_some();
        #[cfg(not(feature = "transcription"))]
        let keep_voice = false;
        if !keep_voice && data.songbird.get(self.instance.guild).is_some() {
            tokio::time::timeout(
                Duration::from_secs(10),
                data.songbird.remove(self.instance.guild),
            )
            .await
            .map_err(|_| NCBError::Timeout {
                operation: "Voice disconnection",
            })?
            .map_err(|_| NCBError::voice_connection("Failed to leave voice channel"))?;
        }
        data.database
            .remove_tts_instance(self.instance.guild)
            .await?;
        let mut sessions = data.tts_data.write().await;
        if sessions
            .get(&self.instance.guild)
            .is_some_and(|session| Arc::ptr_eq(session, self))
        {
            sessions.remove(&self.instance.guild);
        }
        Ok(())
    }
}

struct SpeechProcessor {
    instance: TTSInstance,
    ctx: Context,
    ready: watch::Receiver<bool>,
}

/// Cancelling a job must also stop any audio it has already enqueued.
struct StopTrack(TrackHandle);
impl Drop for StopTrack {
    fn drop(&mut self) {
        let _ = self.0.stop();
    }
}

#[async_trait]
impl Processor<Box<dyn TTSMessage>> for SpeechProcessor {
    async fn process(&mut self, message: Box<dyn TTSMessage>) -> Result<()> {
        self.ready
            .wait_for(|ready| *ready)
            .await
            .map_err(|_| NCBError::SessionStopped)?;
        let mut instance = self.instance.clone();
        let tracks = tokio::time::timeout(
            Duration::from_secs(TTS_TIMEOUT_SECS),
            message.synthesize(&mut instance, &self.ctx),
        )
        .await
        .map_err(|_| NCBError::Timeout {
            operation: "Speech preparation",
        })??;
        for track in tracks {
            let call = self
                .ctx
                .data::<UserData>()
                .songbird
                .get(instance.guild)
                .ok_or_else(|| NCBError::voice_connection("Voice connection is unavailable"))?;
            let handle = call.lock().await.enqueue_with_preload(track, None);
            let track = StopTrack(handle);
            tokio::time::timeout(Duration::from_secs(300), async {
                loop {
                    let info = track
                        .0
                        .get_info()
                        .await
                        .map_err(|_| NCBError::tts_synthesis("Audio track is unavailable"))?;
                    if matches!(info.playing, PlayMode::Errored(_)) {
                        return Err(NCBError::tts_synthesis("Audio playback failed"));
                    }
                    if info.playing.is_done() {
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .map_err(|_| NCBError::Timeout {
                operation: "Audio playback",
            })??;
        }
        self.instance.before_message = instance.before_message;
        Ok(())
    }
}

pub async fn get_session(ctx: &Context, guild: GuildId) -> Option<Arc<TTSSession>> {
    ctx.data::<UserData>()
        .tts_data
        .read()
        .await
        .get(&guild)
        .cloned()
}

#[derive(Debug, PartialEq, Eq)]
pub enum VoicePresence {
    Occupied,
    Empty,
    Unknown,
}

pub fn voice_presence(ctx: &Context, instance: &TTSInstance) -> VoicePresence {
    cached_voice_presence(&ctx.cache, instance)
}

fn cached_voice_presence(cache: &serenity::cache::Cache, instance: &TTSInstance) -> VoicePresence {
    let Some(guild) = cache.guild(instance.guild) else {
        return VoicePresence::Unknown;
    };
    if guild.unavailable() || cache.unavailable_guilds().contains(&instance.guild) {
        return VoicePresence::Unknown;
    }
    // GuildCreate supplies all voice states. Missing member details are treated
    // conservatively as a participant, never as proof of an empty channel.
    let occupied = guild.voice_states.iter().any(|state| {
        state.channel_id == Some(instance.voice_channel)
            && state.user_id != cache.current_user().id
            && !guild
                .members
                .get(&state.user_id)
                .is_some_and(|member| member.user.bot())
    });
    if occupied {
        VoicePresence::Occupied
    } else {
        VoicePresence::Empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::{
        cache::Cache,
        model::{
            event::{GuildCreateEvent, GuildDeleteEvent, VoiceStateUpdateEvent},
            guild::Guild,
            id::ChannelId,
        },
    };

    #[test]
    fn missing_or_unavailable_guild_is_not_an_empty_voice_channel() {
        let cache = Cache::new();
        let instance =
            TTSInstance::new_single(ChannelId::new(1), ChannelId::new(2), GuildId::new(3));
        assert_eq!(
            cached_voice_presence(&cache, &instance),
            VoicePresence::Unknown
        );
        let mut guild = Guild::default();
        guild.id = instance.guild;
        let mut create: GuildCreateEvent =
            serde_json::from_value(serde_json::to_value(guild).unwrap()).unwrap();
        cache.update(&mut create);
        assert_eq!(
            cached_voice_presence(&cache, &instance),
            VoicePresence::Empty
        );
        let mut unavailable: GuildDeleteEvent =
            serde_json::from_value(serde_json::json!({"id": "3", "unavailable": true})).unwrap();
        cache.update(&mut unavailable);
        assert_eq!(
            cached_voice_presence(&cache, &instance),
            VoicePresence::Unknown
        );
    }

    #[test]
    fn voice_state_without_member_details_is_still_a_participant() {
        let cache = Cache::new();
        let instance =
            TTSInstance::new_single(ChannelId::new(1), ChannelId::new(2), GuildId::new(3));
        let mut guild = Guild::default();
        guild.id = instance.guild;
        let mut create: GuildCreateEvent =
            serde_json::from_value(serde_json::to_value(guild).unwrap()).unwrap();
        cache.update(&mut create);
        let mut voice: VoiceStateUpdateEvent = serde_json::from_value(serde_json::json!({
            "guild_id": "3", "channel_id": "2", "user_id": "42", "session_id": "test",
            "deaf": false, "mute": false, "self_deaf": false, "self_mute": false, "self_video": false, "suppress": false
        })).unwrap();
        cache.update(&mut voice);
        assert_eq!(
            cached_voice_presence(&cache, &instance),
            VoicePresence::Occupied
        );
        voice.voice_state.channel_id = None;
        cache.update(&mut voice);
        assert_eq!(
            cached_voice_presence(&cache, &instance),
            VoicePresence::Empty
        );
    }
}
