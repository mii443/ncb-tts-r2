use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};

use crate::transcription::{
    bridge::AudioBridge,
    protocol::{AudioFrame, StreamOpen},
};

const SILENCE_SAMPLES: usize = 320; // 20ms at 16kHz
const SILENCE_TICKS: u16 = 25; // 500ms: hayamimi min_silence + margin
const ORPHAN_LIMIT: usize = 50; // one second of 20ms packets
const WEB_STREAM_CONTEXT_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_WEB_STREAM_CONTEXTS: usize = 1_024;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct GuildId(pub u64);

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct UserId(pub u64);

#[derive(Clone, Debug)]
struct PendingAudio {
    pcm: Vec<i16>,
    captured_at_ms: u64,
}

#[derive(Clone, Debug)]
struct UserStream {
    stream_id: u32,
    user_id: UserId,
    sequence: u32,
    silence_left: u16,
    was_speaking: bool,
    gap_pending: bool,
}

#[derive(Default)]
struct RouterState {
    next_stream_id: u32,
    sessions: HashMap<GuildId, SessionConfig>,
    consents: HashSet<(GuildId, UserId)>,
    revoked: HashSet<(GuildId, UserId)>,
    identities: HashMap<(GuildId, u32), (UserId, String)>,
    avatars: HashMap<(GuildId, UserId), String>,
    by_ssrc: HashMap<(GuildId, u32), UserStream>,
    by_user: HashMap<(GuildId, UserId), u32>,
    orphans: HashMap<(GuildId, u32), VecDeque<PendingAudio>>,
    web_streams: HashMap<u32, WebStreamContext>,
    draining_grants: HashMap<String, WebGrant>,
}

#[derive(Clone, Debug)]
struct SessionConfig {
    voice_channel_id: u64,
    web_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebGrant {
    pub guild_id: GuildId,
    pub voice_channel_id: u64,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebEventContext {
    pub speaker: String,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug)]
struct WebStreamContext {
    guild_id: GuildId,
    voice_channel_id: u64,
    web_token: String,
    user_id: UserId,
    speaker: String,
    avatar_url: Option<String>,
    last_used_at: Instant,
}

pub struct VoiceRouter {
    bridge: Arc<dyn AudioBridge>,
    require_consent: bool,
    state: Mutex<RouterState>,
}

impl VoiceRouter {
    pub fn voice_channel(&self, guild_id: GuildId) -> Option<u64> {
        self.state
            .lock()
            .expect("router poisoned")
            .sessions
            .get(&guild_id)
            .map(|session| session.voice_channel_id)
    }
    pub fn active_sessions(&self) -> Vec<(GuildId, u64)> {
        self.state
            .lock()
            .expect("router poisoned")
            .sessions
            .iter()
            .map(|(guild, session)| (*guild, session.voice_channel_id))
            .collect()
    }

    pub fn new(bridge: Arc<dyn AudioBridge>, require_consent: bool) -> Self {
        Self {
            bridge,
            require_consent,
            state: Mutex::new(RouterState::default()),
        }
    }

    pub fn start_guild(&self, guild_id: GuildId, voice_channel_id: u64) -> String {
        let web_token = random_token();
        self.state.lock().expect("router poisoned").sessions.insert(
            guild_id,
            SessionConfig {
                voice_channel_id,
                web_token: web_token.clone(),
            },
        );
        web_token
    }

    pub fn stop_guild(&self, guild_id: GuildId, reason: &str) -> Option<WebGrant> {
        let mut state = self.state.lock().expect("router poisoned");
        let session = state.sessions.remove(&guild_id)?;
        let grant = WebGrant {
            guild_id,
            voice_channel_id: session.voice_channel_id,
            token: session.web_token,
        };
        end_guild_streams(&*self.bridge, &mut state, guild_id, reason);
        state
            .draining_grants
            .insert(grant.token.clone(), grant.clone());
        Some(grant)
    }

    pub fn abort_guild(&self, guild_id: GuildId, reason: &str) {
        let mut state = self.state.lock().expect("router poisoned");
        let web_token = state
            .sessions
            .remove(&guild_id)
            .map(|session| session.web_token);
        end_guild_streams(&*self.bridge, &mut state, guild_id, reason);
        if let Some(web_token) = web_token {
            state
                .web_streams
                .retain(|_, context| context.web_token != web_token);
        }
    }

    pub fn finish_web_drain(&self, grant: &WebGrant) {
        let mut state = self.state.lock().expect("router poisoned");
        if state.draining_grants.get(&grant.token) != Some(grant) {
            return;
        }
        state.draining_grants.remove(&grant.token);
        state
            .web_streams
            .retain(|_, context| context.web_token != grant.token);
    }

    #[cfg(test)]
    fn speaking_state(&self, guild_id: GuildId, ssrc: u32, user_id: UserId, speaker: String) {
        self.speaking_state_with_avatar(guild_id, ssrc, user_id, speaker, None);
    }

    pub fn speaking_state_with_avatar(
        &self,
        guild_id: GuildId,
        ssrc: u32,
        user_id: UserId,
        speaker: String,
        avatar_url: Option<String>,
    ) {
        self.speaking_state_for_session(guild_id, None, ssrc, user_id, speaker, avatar_url);
    }

    pub fn speaking_state_for_session(
        &self,
        guild_id: GuildId,
        token: Option<&str>,
        ssrc: u32,
        user_id: UserId,
        speaker: String,
        avatar_url: Option<String>,
    ) {
        let mut state = self.state.lock().expect("router poisoned");
        if state
            .sessions
            .get(&guild_id)
            .is_none_or(|session| token.is_some_and(|token| session.web_token != token))
        {
            return;
        }
        if let Some(avatar_url) = avatar_url {
            state.avatars.insert((guild_id, user_id), avatar_url);
        }
        let avatar_url = state.avatars.get(&(guild_id, user_id)).cloned();
        for context in state
            .web_streams
            .values_mut()
            .filter(|context| context.guild_id == guild_id && context.user_id == user_id)
        {
            context.speaker.clone_from(&speaker);
            context.avatar_url.clone_from(&avatar_url);
        }
        state
            .identities
            .insert((guild_id, ssrc), (user_id, speaker.clone()));
        if !is_consented(&state, self.require_consent, guild_id, user_id) {
            state.orphans.remove(&(guild_id, ssrc));
            return;
        }
        open_user_stream(&*self.bridge, &mut state, guild_id, ssrc, user_id, speaker);
    }

    pub fn consent(&self, guild_id: GuildId, user_id: UserId) -> bool {
        let mut state = self.state.lock().expect("router poisoned");
        if !state.sessions.contains_key(&guild_id) {
            return false;
        }
        state.consents.insert((guild_id, user_id));
        state.revoked.remove(&(guild_id, user_id));
        let identities: Vec<_> = state
            .identities
            .iter()
            .filter(|((guild, _), (user, _))| *guild == guild_id && *user == user_id)
            .map(|((_, ssrc), (_, speaker))| (*ssrc, speaker.clone()))
            .collect();
        for (ssrc, speaker) in identities {
            open_user_stream(&*self.bridge, &mut state, guild_id, ssrc, user_id, speaker);
        }
        true
    }

    pub fn revoke(&self, guild_id: GuildId, user_id: UserId) -> bool {
        let mut state = self.state.lock().expect("router poisoned");
        if !state.sessions.contains_key(&guild_id) {
            return false;
        }
        state.consents.remove(&(guild_id, user_id));
        state.revoked.insert((guild_id, user_id));
        disconnect_user_locked(
            &*self.bridge,
            &mut state,
            guild_id,
            user_id,
            "consent_revoked",
        );
        let ssrcs: Vec<_> = state
            .identities
            .iter()
            .filter(|((guild, _), (user, _))| *guild == guild_id && *user == user_id)
            .map(|((_, ssrc), _)| *ssrc)
            .collect();
        for ssrc in ssrcs {
            state.orphans.remove(&(guild_id, ssrc));
        }
        true
    }

    pub fn requires_consent(&self) -> bool {
        self.require_consent
    }

    pub fn is_active(&self, guild_id: GuildId) -> bool {
        self.state
            .lock()
            .expect("router poisoned")
            .sessions
            .contains_key(&guild_id)
    }

    pub fn disconnect_user(&self, guild_id: GuildId, user_id: UserId) {
        let mut state = self.state.lock().expect("router poisoned");
        let ssrcs: Vec<_> = state
            .identities
            .iter()
            .filter(|((guild, _), (user, _))| *guild == guild_id && *user == user_id)
            .map(|((_, ssrc), _)| *ssrc)
            .collect();
        for ssrc in ssrcs {
            state.identities.remove(&(guild_id, ssrc));
            state.orphans.remove(&(guild_id, ssrc));
        }
        state.avatars.remove(&(guild_id, user_id));
        disconnect_user_locked(&*self.bridge, &mut state, guild_id, user_id, "left");
    }

    pub fn active_streams(&self, guild_id: GuildId) -> usize {
        self.state
            .lock()
            .expect("router poisoned")
            .by_ssrc
            .keys()
            .filter(|(guild, _)| *guild == guild_id)
            .count()
    }

    pub fn web_token(&self, guild_id: GuildId) -> Option<String> {
        self.state
            .lock()
            .expect("router poisoned")
            .sessions
            .get(&guild_id)
            .map(|session| session.web_token.clone())
    }

    #[cfg(test)]
    fn avatar_url(&self, guild_id: GuildId, user_id: UserId) -> Option<String> {
        self.state
            .lock()
            .expect("router poisoned")
            .avatars
            .get(&(guild_id, user_id))
            .cloned()
    }

    pub fn web_event_context(&self, grant: &WebGrant, stream_id: u32) -> Option<WebEventContext> {
        let mut state = self.state.lock().expect("router poisoned");
        let context = state.web_streams.get_mut(&stream_id)?;
        if !(context.guild_id == grant.guild_id
            && context.voice_channel_id == grant.voice_channel_id
            && context.web_token == grant.token)
        {
            return None;
        }
        context.last_used_at = Instant::now();
        Some(WebEventContext {
            speaker: context.speaker.clone(),
            avatar_url: context.avatar_url.clone(),
        })
    }

    pub fn web_grant(&self, token: &str) -> Option<WebGrant> {
        let state = self.state.lock().expect("router poisoned");
        state
            .sessions
            .iter()
            .find(|(_, session)| session.web_token == token)
            .map(|(guild_id, session)| WebGrant {
                guild_id: *guild_id,
                voice_channel_id: session.voice_channel_id,
                token: session.web_token.clone(),
            })
            .or_else(|| state.draining_grants.get(token).cloned())
    }

    pub fn is_web_grant_active(&self, grant: &WebGrant) -> bool {
        let state = self.state.lock().expect("router poisoned");
        state.sessions.get(&grant.guild_id).is_some_and(|session| {
            session.voice_channel_id == grant.voice_channel_id && session.web_token == grant.token
        }) || state.draining_grants.get(&grant.token) == Some(grant)
    }
}

fn end_guild_streams(
    bridge: &dyn AudioBridge,
    state: &mut RouterState,
    guild_id: GuildId,
    reason: &str,
) {
    state.consents.retain(|(guild, _)| *guild != guild_id);
    state.revoked.retain(|(guild, _)| *guild != guild_id);
    state.identities.retain(|(guild, _), _| *guild != guild_id);
    state.avatars.retain(|(guild, _), _| *guild != guild_id);
    let ids: Vec<_> = state
        .by_ssrc
        .iter()
        .filter(|((guild, _), _)| *guild == guild_id)
        .map(|(key, stream)| (*key, stream.user_id, stream.stream_id))
        .collect();
    for (key, user_id, stream_id) in ids {
        state.by_ssrc.remove(&key);
        state.by_user.remove(&(guild_id, user_id));
        touch_web_stream(state, stream_id);
        bridge.end(stream_id, reason);
    }
    state.orphans.retain(|(guild, _), _| *guild != guild_id);
}

fn prune_web_streams(state: &mut RouterState) {
    let active: HashSet<_> = state
        .by_ssrc
        .values()
        .map(|stream| stream.stream_id)
        .collect();
    let draining: HashSet<_> = state.draining_grants.keys().cloned().collect();
    let now = Instant::now();
    state.web_streams.retain(|stream_id, context| {
        active.contains(stream_id)
            || draining.contains(&context.web_token)
            || now.duration_since(context.last_used_at) < WEB_STREAM_CONTEXT_TTL
    });

    while state.web_streams.len() >= MAX_WEB_STREAM_CONTEXTS {
        let Some(oldest) = state
            .web_streams
            .iter()
            .filter(|(stream_id, _)| !active.contains(stream_id))
            .min_by_key(|(_, context)| context.last_used_at)
            .map(|(stream_id, _)| *stream_id)
        else {
            break;
        };
        state.web_streams.remove(&oldest);
    }
}

fn touch_web_stream(state: &mut RouterState, stream_id: u32) {
    if let Some(context) = state.web_streams.get_mut(&stream_id) {
        context.last_used_at = Instant::now();
    }
}

fn open_user_stream(
    bridge: &dyn AudioBridge,
    state: &mut RouterState,
    guild_id: GuildId,
    ssrc: u32,
    user_id: UserId,
    speaker: String,
) {
    let Some(session) = state.sessions.get(&guild_id).cloned() else {
        return;
    };
    if let Some(old_ssrc) = state.by_user.get(&(guild_id, user_id)).copied() {
        if old_ssrc != ssrc {
            state.by_user.remove(&(guild_id, user_id));
            if let Some(old) = state.by_ssrc.remove(&(guild_id, old_ssrc)) {
                touch_web_stream(state, old.stream_id);
                bridge.end(old.stream_id, "ssrc_changed");
            }
        } else {
            return;
        }
    }
    prune_web_streams(state);
    if state.web_streams.len() >= MAX_WEB_STREAM_CONTEXTS {
        return;
    }
    state.next_stream_id = state.next_stream_id.wrapping_add(1).max(1);
    let stream_id = state.next_stream_id;
    let stream = UserStream {
        stream_id,
        user_id,
        sequence: 0,
        silence_left: 0,
        was_speaking: false,
        gap_pending: false,
    };
    state.by_user.insert((guild_id, user_id), ssrc);
    state.by_ssrc.insert((guild_id, ssrc), stream);
    let avatar_url = state.avatars.get(&(guild_id, user_id)).cloned();
    state.web_streams.insert(
        stream_id,
        WebStreamContext {
            guild_id,
            voice_channel_id: session.voice_channel_id,
            web_token: session.web_token.clone(),
            user_id,
            speaker: speaker.clone(),
            avatar_url,
            last_used_at: Instant::now(),
        },
    );
    let mut metadata = Map::new();
    metadata.insert("guild_id".into(), Value::String(guild_id.0.to_string()));
    metadata.insert(
        "channel_id".into(),
        Value::String(session.voice_channel_id.to_string()),
    );
    bridge.open(StreamOpen::new(
        stream_id,
        user_id.0.to_string(),
        speaker,
        now_ms(),
        metadata,
    ));
    if let Some(mut pending) = state.orphans.remove(&(guild_id, ssrc)) {
        while let Some(audio) = pending.pop_front() {
            if let Some(stream) = state.by_ssrc.get_mut(&(guild_id, ssrc)) {
                send_pcm(bridge, stream, audio.pcm, audio.captured_at_ms);
            }
        }
    }
}

fn disconnect_user_locked(
    bridge: &dyn AudioBridge,
    state: &mut RouterState,
    guild_id: GuildId,
    user_id: UserId,
    reason: &str,
) {
    let Some(ssrc) = state.by_user.remove(&(guild_id, user_id)) else {
        return;
    };
    if let Some(stream) = state.by_ssrc.remove(&(guild_id, ssrc)) {
        touch_web_stream(state, stream.stream_id);
        bridge.end(stream.stream_id, reason);
    }
}

impl VoiceRouter {
    pub fn voice_tick(
        &self,
        guild_id: GuildId,
        speaking: impl IntoIterator<Item = (u32, Vec<i16>)>,
        silent: impl IntoIterator<Item = u32>,
    ) {
        let mut state = self.state.lock().expect("router poisoned");
        if !state.sessions.contains_key(&guild_id) {
            return;
        }
        let captured_at_ms = now_ms();
        let guild_has_revoked_user = state.revoked.iter().any(|(guild, _)| *guild == guild_id);
        for (ssrc, pcm) in speaking {
            if self.require_consent || guild_has_revoked_user {
                let Some((user_id, _)) = state.identities.get(&(guild_id, ssrc)) else {
                    continue;
                };
                if !is_consented(&state, self.require_consent, guild_id, *user_id) {
                    continue;
                }
            }
            if let Some(stream) = state.by_ssrc.get_mut(&(guild_id, ssrc)) {
                stream.was_speaking = true;
                stream.silence_left = SILENCE_TICKS;
                send_pcm(&*self.bridge, stream, pcm, captured_at_ms);
            } else {
                let pending = state.orphans.entry((guild_id, ssrc)).or_default();
                if pending.len() == ORPHAN_LIMIT {
                    pending.pop_front();
                }
                pending.push_back(PendingAudio {
                    pcm,
                    captured_at_ms,
                });
            }
        }
        for ssrc in silent {
            let Some(stream) = state.by_ssrc.get_mut(&(guild_id, ssrc)) else {
                continue;
            };
            if stream.silence_left > 0 {
                send_pcm(
                    &*self.bridge,
                    stream,
                    vec![0; SILENCE_SAMPLES],
                    captured_at_ms,
                );
                stream.silence_left -= 1;
                if stream.silence_left == 0 && stream.was_speaking {
                    self.bridge.idle(stream.stream_id);
                    stream.was_speaking = false;
                }
            }
        }
    }
}

fn is_consented(
    state: &RouterState,
    require_consent: bool,
    guild_id: GuildId,
    user_id: UserId,
) -> bool {
    !state.revoked.contains(&(guild_id, user_id))
        && (!require_consent || state.consents.contains(&(guild_id, user_id)))
}

fn send_pcm(bridge: &dyn AudioBridge, stream: &mut UserStream, pcm: Vec<i16>, captured_at_ms: u64) {
    if stream.gap_pending {
        bridge.gap(stream.stream_id, "bridge_queue_overflow");
        stream.gap_pending = false;
    }
    let mut bytes = Vec::with_capacity(pcm.len() * 2);
    for sample in pcm {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let accepted = bridge.audio(AudioFrame {
        stream_id: stream.stream_id,
        sequence: stream.sequence,
        captured_at_ms,
        pcm: bytes,
    });
    stream.sequence = stream.sequence.wrapping_add(1);
    if !accepted {
        stream.gap_pending = true;
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeBridge {
        events: Mutex<Vec<String>>,
        audio: Mutex<Vec<AudioFrame>>,
        accept: Mutex<bool>,
    }

    #[async_trait::async_trait]
    impl AudioBridge for FakeBridge {
        fn open(&self, value: StreamOpen) {
            self.events
                .lock()
                .unwrap()
                .push(format!("open:{}:{}", value.stream_id, value.speaker_id));
        }
        fn audio(&self, value: AudioFrame) -> bool {
            self.audio.lock().unwrap().push(value);
            *self.accept.lock().unwrap()
        }
        fn idle(&self, id: u32) {
            self.events.lock().unwrap().push(format!("idle:{id}"));
        }
        fn gap(&self, id: u32, reason: &str) {
            self.events
                .lock()
                .unwrap()
                .push(format!("gap:{id}:{reason}"));
        }
        fn end(&self, id: u32, reason: &str) {
            self.events
                .lock()
                .unwrap()
                .push(format!("end:{id}:{reason}"));
        }
    }

    fn setup() -> (Arc<FakeBridge>, VoiceRouter) {
        let bridge = Arc::new(FakeBridge::default());
        *bridge.accept.lock().unwrap() = true;
        let router = VoiceRouter::new(bridge.clone(), false);
        router.start_guild(GuildId(1), 10);
        (bridge, router)
    }

    #[test]
    fn keeps_simultaneous_users_on_distinct_streams() {
        let (bridge, router) = setup();
        router.speaking_state(GuildId(1), 100, UserId(11), "Alice".into());
        router.speaking_state(GuildId(1), 200, UserId(22), "Bob".into());
        router.voice_tick(GuildId(1), [(100, vec![1; 320]), (200, vec![2; 320])], []);
        let frames = bridge.audio.lock().unwrap();
        assert_eq!(frames.len(), 2);
        assert_ne!(frames[0].stream_id, frames[1].stream_id);
        assert_eq!(&frames[0].pcm[..2], &1i16.to_le_bytes());
        assert_eq!(&frames[1].pcm[..2], &2i16.to_le_bytes());
    }

    #[test]
    fn replays_orphan_only_after_ssrc_identity_arrives() {
        let (bridge, router) = setup();
        router.voice_tick(GuildId(1), [(777, vec![7; 320])], []);
        assert!(bridge.audio.lock().unwrap().is_empty());
        router.speaking_state(GuildId(1), 777, UserId(77), "Late".into());
        assert_eq!(bridge.audio.lock().unwrap().len(), 1);
    }

    #[test]
    fn sends_trailing_silence_then_idle() {
        let (bridge, router) = setup();
        router.speaking_state(GuildId(1), 100, UserId(11), "Alice".into());
        router.voice_tick(GuildId(1), [(100, vec![1; 320])], []);
        for _ in 0..SILENCE_TICKS {
            router.voice_tick(GuildId(1), [], [100]);
        }
        let frames = bridge.audio.lock().unwrap();
        assert_eq!(frames.len(), 1 + SILENCE_TICKS as usize);
        assert!(frames.last().unwrap().pcm.iter().all(|byte| *byte == 0));
        assert!(bridge
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.starts_with("idle:")));
    }

    #[test]
    fn marks_gap_after_bridge_backpressure() {
        let (bridge, router) = setup();
        router.speaking_state(GuildId(1), 100, UserId(11), "Alice".into());
        *bridge.accept.lock().unwrap() = false;
        router.voice_tick(GuildId(1), [(100, vec![1; 320])], []);
        *bridge.accept.lock().unwrap() = true;
        router.voice_tick(GuildId(1), [(100, vec![2; 320])], []);
        assert!(bridge
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.contains("bridge_queue_overflow")));
    }

    #[test]
    fn explicit_consent_discards_audio_until_granted_and_stops_on_revoke() {
        let bridge = Arc::new(FakeBridge::default());
        *bridge.accept.lock().unwrap() = true;
        let router = VoiceRouter::new(bridge.clone(), true);
        router.start_guild(GuildId(1), 10);
        router.speaking_state(GuildId(1), 100, UserId(11), "Alice".into());
        router.voice_tick(GuildId(1), [(100, vec![1; 320])], []);
        assert!(bridge.audio.lock().unwrap().is_empty());
        assert!(router.consent(GuildId(1), UserId(11)));
        router.voice_tick(GuildId(1), [(100, vec![2; 320])], []);
        assert_eq!(bridge.audio.lock().unwrap().len(), 1);
        assert!(router.revoke(GuildId(1), UserId(11)));
        router.voice_tick(GuildId(1), [(100, vec![3; 320])], []);
        assert_eq!(bridge.audio.lock().unwrap().len(), 1);
        assert!(bridge
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.contains("consent_revoked")));
    }

    #[test]
    fn consent_is_enabled_by_default_but_revoke_still_stops_audio() {
        let (bridge, router) = setup();
        router.speaking_state(GuildId(1), 100, UserId(11), "Alice".into());
        router.voice_tick(GuildId(1), [(100, vec![1; 320])], []);
        assert_eq!(bridge.audio.lock().unwrap().len(), 1);

        assert!(router.revoke(GuildId(1), UserId(11)));
        router.voice_tick(GuildId(1), [(100, vec![2; 320])], []);
        assert_eq!(bridge.audio.lock().unwrap().len(), 1);

        assert!(router.consent(GuildId(1), UserId(11)));
        router.voice_tick(GuildId(1), [(100, vec![3; 320])], []);
        assert_eq!(bridge.audio.lock().unwrap().len(), 2);
    }

    #[test]
    fn web_grant_drains_then_expires_with_the_voice_session() {
        let (_bridge, router) = setup();
        let token = router.web_token(GuildId(1)).unwrap();
        assert_eq!(token.len(), 64);
        let grant = router.web_grant(&token).unwrap();
        assert_eq!(grant.guild_id, GuildId(1));
        assert_eq!(grant.voice_channel_id, 10);
        assert!(router.is_web_grant_active(&grant));

        let draining_grant = router.stop_guild(GuildId(1), "test").unwrap();
        assert_eq!(router.web_grant(&token), Some(grant.clone()));
        assert!(router.is_web_grant_active(&grant));

        router.finish_web_drain(&draining_grant);
        assert!(router.web_grant(&token).is_none());
        assert!(!router.is_web_grant_active(&grant));
    }

    #[test]
    fn discord_avatar_follows_the_voice_membership_lifecycle() {
        let (_bridge, router) = setup();
        let grant = router
            .web_grant(&router.web_token(GuildId(1)).unwrap())
            .unwrap();
        router.speaking_state_with_avatar(
            GuildId(1),
            100,
            UserId(11),
            "Alice".into(),
            Some("https://cdn.discordapp.com/avatars/11/example.webp".into()),
        );
        assert_eq!(
            router.avatar_url(GuildId(1), UserId(11)).as_deref(),
            Some("https://cdn.discordapp.com/avatars/11/example.webp")
        );

        router.disconnect_user(GuildId(1), UserId(11));
        assert!(router.avatar_url(GuildId(1), UserId(11)).is_none());
        assert_eq!(
            router
                .web_event_context(&grant, 1)
                .unwrap()
                .avatar_url
                .as_deref(),
            Some("https://cdn.discordapp.com/avatars/11/example.webp")
        );
    }

    #[test]
    fn inactive_web_stream_context_expires_but_active_stream_is_retained() {
        let (_bridge, router) = setup();
        let grant = router
            .web_grant(&router.web_token(GuildId(1)).unwrap())
            .unwrap();
        router.speaking_state(GuildId(1), 100, UserId(11), "Alice".into());
        {
            let mut state = router.state.lock().unwrap();
            state.web_streams.get_mut(&1).unwrap().last_used_at =
                Instant::now() - WEB_STREAM_CONTEXT_TTL - Duration::from_secs(1);
            prune_web_streams(&mut state);
        }
        assert!(router.web_event_context(&grant, 1).is_some());

        {
            let mut state = router.state.lock().unwrap();
            state.web_streams.get_mut(&1).unwrap().last_used_at =
                Instant::now() - WEB_STREAM_CONTEXT_TTL - Duration::from_secs(1);
        }
        router.disconnect_user(GuildId(1), UserId(11));
        {
            let mut state = router.state.lock().unwrap();
            assert!(state.web_streams[&1].last_used_at.elapsed() < Duration::from_secs(1));
            state.web_streams.get_mut(&1).unwrap().last_used_at =
                Instant::now() - WEB_STREAM_CONTEXT_TTL - Duration::from_secs(1);
            prune_web_streams(&mut state);
        }
        assert!(router.web_event_context(&grant, 1).is_none());
    }

    #[test]
    fn web_stream_contexts_have_a_hard_limit() {
        let (bridge, router) = setup();
        for id in 1..=(MAX_WEB_STREAM_CONTEXTS as u64 + 1) {
            router.speaking_state(
                GuildId(1),
                u32::try_from(id).unwrap(),
                UserId(id),
                format!("speaker-{id}"),
            );
        }

        let state = router.state.lock().unwrap();
        assert_eq!(state.web_streams.len(), MAX_WEB_STREAM_CONTEXTS);
        assert_eq!(state.by_ssrc.len(), MAX_WEB_STREAM_CONTEXTS);
        assert_eq!(
            bridge
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.starts_with("open:"))
                .count(),
            MAX_WEB_STREAM_CONTEXTS
        );
    }
    #[test]
    fn late_identity_lookup_cannot_open_a_stream_in_a_replacement_session() {
        let bridge = Arc::new(FakeBridge::default());
        let router = VoiceRouter::new(bridge.clone(), false);
        let old = router.start_guild(GuildId(1), 2);
        router.abort_guild(GuildId(1), "test");
        router.start_guild(GuildId(1), 3);
        router.speaking_state_for_session(
            GuildId(1),
            Some(&old),
            42,
            UserId(4),
            "old".into(),
            None,
        );
        assert_eq!(router.active_streams(GuildId(1)), 0);
        assert!(bridge.events.lock().unwrap().is_empty());
    }
}
