use super::*;
use crate::transcription::{
    bridge::AudioBridge,
    protocol::{AudioFrame, StreamOpen},
};
use serenity::all::{Guild, GuildCreateEvent, Member, VoiceStateUpdateEvent};

#[derive(Default)]
struct RecordingBridge {
    opens: Mutex<Vec<StreamOpen>>,
    frames: Mutex<Vec<AudioFrame>>,
    ends: Mutex<Vec<u32>>,
}

#[async_trait]
impl AudioBridge for RecordingBridge {
    fn open(&self, stream: StreamOpen) {
        self.opens.lock().unwrap().push(stream);
    }
    fn audio(&self, frame: AudioFrame) -> bool {
        self.frames.lock().unwrap().push(frame);
        true
    }
    fn idle(&self, _: u32) {}
    fn gap(&self, _: u32, _: &str) {}
    fn end(&self, stream_id: u32, _: &str) {
        self.ends.lock().unwrap().push(stream_id);
    }
}

fn voice_state(cache: &Cache, user: u64, channel: Option<u64>) -> VoiceState {
    let mut event: VoiceStateUpdateEvent = serde_json::from_value(serde_json::json!({
        "guild_id": "123", "channel_id": channel.map(|id| id.to_string()),
        "user_id": user.to_string(), "session_id": "test",
        "deaf": false, "mute": false, "self_deaf": false, "self_mute": false,
        "self_video": false, "suppress": false,
    }))
    .unwrap();
    cache.update(&mut event);
    event.voice_state
}

fn fixture(require_consent: bool) -> (Arc<RecordingBridge>, DiscordVoiceReceiver) {
    let bridge = Arc::new(RecordingBridge::default());
    let cache = Arc::new(Cache::new());
    let mut guild = Guild::default();
    guild.id = SerenityGuildId::new(123);
    for (id, name) in [(11, "Alice"), (22, "Bob")] {
        let mut member = Member::default();
        member.user.id = SerenityUserId::new(id);
        member.user.name = name.parse().unwrap();
        guild.members.insert(member);
    }
    let mut create: GuildCreateEvent =
        serde_json::from_value(serde_json::to_value(guild).unwrap()).unwrap();
    cache.update(&mut create);
    voice_state(&cache, cache.current_user().id.get(), Some(789));
    voice_state(&cache, 11, Some(789));
    voice_state(&cache, 22, Some(789));
    let receiver = DiscordVoiceReceiver {
        guild_id: SerenityGuildId::new(123),
        router: Arc::new(VoiceRouter::new(bridge.clone(), require_consent)),
        http: Arc::new(Http::without_token()),
        cache,
        state: Arc::new(Mutex::new(ReceiverState {
            channel: Some(ChannelId::new(789)),
            ..Default::default()
        })),
    };
    (bridge, receiver)
}

async fn speaking(receiver: &DiscordVoiceReceiver, user_id: u64, ssrc: u32) {
    let speaking = serde_json::from_value(serde_json::json!({
        "user_id": user_id.to_string(), "ssrc": ssrc, "speaking": 1, "delay": 0,
    }))
    .unwrap();
    receiver
        .act(&EventContext::SpeakingStateUpdate(speaking))
        .await;
}

#[tokio::test]
async fn tts_first_then_transcription_and_restart_reuse_speaker_identity() {
    let (bridge, receiver) = fixture(false);
    // Discord announces the SSRC once, while only TTS is active.
    speaking(&receiver, 11, 100).await;
    receiver.voice_tick([(100, vec![1; 320])], []);
    assert!(bridge.opens.lock().unwrap().is_empty());
    assert!(bridge.frames.lock().unwrap().is_empty());
    let first = receiver.router.start_guild(GuildId(123), 789);
    receiver.voice_tick([(100, vec![2; 320])], []);
    assert_eq!(receiver.router.active_streams(GuildId(123)), 1);
    assert_eq!(bridge.opens.lock().unwrap()[0].speaker_id, "11");
    assert_eq!(bridge.opens.lock().unwrap()[0].speaker, "Alice");
    receiver.router.stop_guild(GuildId(123), "stopped");
    receiver.voice_tick([(100, vec![3; 320])], []);
    let second = receiver.router.start_guild(GuildId(123), 789);
    assert_ne!(first, second);
    // No new SpeakingStateUpdate or reconnect occurs on restart.
    receiver.voice_tick([(100, vec![4; 320])], []);
    let frames = bridge.frames.lock().unwrap();
    assert_eq!(
        frames.len(),
        2,
        "pre-start/stopped PCM must not be buffered"
    );
    assert_ne!(frames[0].stream_id, frames[1].stream_id);
    assert_eq!(frames[0].pcm, 2i16.to_le_bytes().repeat(320));
    assert_eq!(frames[1].pcm, 4i16.to_le_bytes().repeat(320));
    assert_eq!(
        bridge.ends.lock().unwrap().as_slice(),
        &[frames[0].stream_id]
    );
}

#[tokio::test]
async fn transcription_first_preserves_simultaneous_participant_streams() {
    let (bridge, receiver) = fixture(false);
    receiver.router.start_guild(GuildId(123), 789);
    speaking(&receiver, 11, 100).await;
    speaking(&receiver, 22, 200).await;
    receiver.voice_tick([(100, vec![1; 320]), (200, vec![2; 320])], []);
    let frames = bridge.frames.lock().unwrap();
    assert_eq!(frames.len(), 2);
    assert_ne!(frames[0].stream_id, frames[1].stream_id);
    assert_eq!(frames[0].pcm, 1i16.to_le_bytes().repeat(320));
    assert_eq!(frames[1].pcm, 2i16.to_le_bytes().repeat(320));
}

#[tokio::test]
async fn cached_speakers_still_require_consent_and_respect_revocation() {
    let (bridge, receiver) = fixture(true);
    speaking(&receiver, 11, 100).await;
    receiver.router.start_guild(GuildId(123), 789);
    receiver.voice_tick([(100, vec![1; 320])], []);
    assert!(bridge.opens.lock().unwrap().is_empty());
    receiver.router.consent(GuildId(123), UserId(11));
    receiver.voice_tick([(100, vec![2; 320])], []);
    receiver.router.revoke(GuildId(123), UserId(11));
    speaking(&receiver, 11, 100).await;
    receiver.voice_tick([(100, vec![3; 320])], []);
    let frames = bridge.frames.lock().unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].pcm, 2i16.to_le_bytes().repeat(320));
}

#[tokio::test]
async fn client_disconnect_while_only_tts_is_active_forgets_ssrc() {
    let (bridge, receiver) = fixture(false);
    speaking(&receiver, 11, 100).await;
    receiver
        .act(&EventContext::ClientDisconnect(
            serde_json::from_value(serde_json::json!({"user_id": "11"})).unwrap(),
        ))
        .await;
    receiver.router.start_guild(GuildId(123), 789);
    receiver.voice_tick([(100, vec![1; 320])], []);
    assert!(bridge.opens.lock().unwrap().is_empty());
    assert!(bridge.frames.lock().unwrap().is_empty());
}

#[tokio::test]
async fn gateway_departure_during_tts_and_channel_change_clear_cached_speakers() {
    let (bridge, receiver) = fixture(false);
    let call = Arc::new(tokio::sync::Mutex::new(Call::standalone_from_config(
        receiver.guild_id,
        receiver.cache.current_user().id,
        super::super::voice_config(true),
    )));
    let registry = ReceiverRegistry(Mutex::new(HashMap::from([(
        receiver.guild_id,
        Registration {
            call: Arc::downgrade(&call),
            receiver: receiver.clone(),
        },
    )])));
    speaking(&receiver, 11, 100).await;
    let departed = voice_state(&receiver.cache, 11, None);
    registry.voice_state_update(receiver.cache.current_user().id, &departed);
    receiver.router.start_guild(GuildId(123), 789);
    receiver.voice_tick([(100, vec![1; 320])], []);
    assert!(bridge.frames.lock().unwrap().is_empty());
    speaking(&receiver, 22, 200).await;
    receiver.router.stop_guild(GuildId(123), "stopped");
    receiver.set_channel(Some(ChannelId::new(999)));
    voice_state(
        &receiver.cache,
        receiver.cache.current_user().id.get(),
        Some(999),
    );
    receiver.router.start_guild(GuildId(123), 999);
    receiver.voice_tick([(200, vec![2; 320])], []);
    assert!(bridge.opens.lock().unwrap().is_empty());
    assert!(bridge.frames.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ssrc_reassignment_does_not_resurrect_the_previous_mapping() {
    let (bridge, receiver) = fixture(false);
    speaking(&receiver, 11, 100).await;
    receiver.router.start_guild(GuildId(123), 789);
    receiver.voice_tick([(100, vec![1; 320])], []);
    speaking(&receiver, 11, 200).await;
    receiver.voice_tick([(200, vec![2; 320])], []);
    speaking(&receiver, 22, 100).await;
    receiver.voice_tick([(100, vec![3; 320]), (200, vec![4; 320])], []);
    assert_eq!(receiver.router.active_streams(GuildId(123)), 2);
    assert_eq!(bridge.frames.lock().unwrap().len(), 4);
    assert_eq!(bridge.opens.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn voice_manager_prepares_decoding_before_tts_creates_the_first_call() {
    use serenity::gateway::VoiceGatewayManager;
    use songbird::driver::{Channels, DecodeConfig, DecodeMode, SampleRate};
    let mut config: crate::config::Config = toml::from_str(
        r#"
        prefix = "t!"
        token = "unused"
        application_id = 1
        redis_url = "redis://localhost/"
    "#,
    )
    .unwrap();
    let manager = super::super::voice_manager(&config);
    manager.initialise_client_data(1, SerenityUserId::new(1));
    let (sender, _receiver) = futures::channel::mpsc::unbounded();
    manager.register_shard(0, sender).await;
    let call = manager.get_or_insert(SerenityGuildId::new(123));
    assert_eq!(
        call.lock().await.config().decode_mode,
        DecodeMode::Decode(DecodeConfig::new(Channels::Mono, SampleRate::Hz16000))
    );
    config.transcription.enabled = false;
    let manager = super::super::voice_manager(&config);
    manager.initialise_client_data(1, SerenityUserId::new(1));
    let (sender, _receiver) = futures::channel::mpsc::unbounded();
    manager.register_shard(0, sender).await;
    let call = manager.get_or_insert(SerenityGuildId::new(123));
    assert_eq!(call.lock().await.config().decode_mode, DecodeMode::Pass);
}
