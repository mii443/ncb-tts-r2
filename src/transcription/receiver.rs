use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use serenity::{
    all::{ChannelId, Context, GuildId as SerenityGuildId, UserId as SerenityUserId, VoiceState},
    async_trait,
    cache::Cache,
    http::Http,
};
use songbird::{events::EventHandler as VoiceEventHandler, Call, CoreEvent, Event, EventContext};

use super::router::{GuildId, UserId, VoiceRouter};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct ReceiverRegistry(Mutex<HashMap<SerenityGuildId, Registration>>);

struct Registration {
    call: Weak<tokio::sync::Mutex<Call>>,
    receiver: DiscordVoiceReceiver,
}

impl ReceiverRegistry {
    /// The caller holds the guild setup lock so registration completes before joining.
    pub async fn install(
        &self,
        ctx: &Context,
        guild_id: SerenityGuildId,
        channel: ChannelId,
        call: &Arc<tokio::sync::Mutex<Call>>,
        router: &Arc<VoiceRouter>,
    ) {
        let receiver = {
            let mut entries = self.0.lock().expect("receiver registry poisoned");
            entries.retain(|_, entry| {
                if entry.call.strong_count() == 0 {
                    entry.receiver.set_channel(None);
                    false
                } else {
                    true
                }
            });
            if let Some(entry) = entries.get(&guild_id) {
                if entry.call.ptr_eq(&Arc::downgrade(call)) {
                    entry.receiver.set_channel(Some(channel));
                    return;
                }
                // An event from a retired Call must not populate the replacement.
                entry.receiver.set_channel(None);
            }
            let receiver = DiscordVoiceReceiver {
                guild_id,
                router: router.clone(),
                http: ctx.http.clone(),
                cache: ctx.cache.clone(),
                state: Arc::new(Mutex::new(ReceiverState {
                    channel: Some(channel),
                    ..Default::default()
                })),
            };
            entries.insert(
                guild_id,
                Registration {
                    call: Arc::downgrade(call),
                    receiver: receiver.clone(),
                },
            );
            receiver
        };
        let mut call = call.lock().await;
        call.add_global_event(CoreEvent::SpeakingStateUpdate.into(), receiver.clone());
        call.add_global_event(CoreEvent::VoiceTick.into(), receiver.clone());
        call.add_global_event(CoreEvent::ClientDisconnect.into(), receiver.clone());
        call.add_global_event(CoreEvent::DriverDisconnect.into(), receiver);
    }

    pub fn voice_state_update(&self, bot_user: SerenityUserId, voice: &VoiceState) {
        let entries = self.0.lock().expect("receiver registry poisoned");
        let Some(entry) = voice.guild_id.and_then(|guild| entries.get(&guild)) else {
            return;
        };
        let receiver = &entry.receiver;
        let mut state = receiver.state.lock().expect("receiver poisoned");
        if voice.channel_id != state.channel {
            if voice.user_id == bot_user {
                receiver.clear(&mut state);
                state.channel = None;
            } else {
                receiver.forget_user(&mut state, UserId(voice.user_id.get()));
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Speaker {
    user_id: UserId,
    name: String,
    avatar_url: Option<String>,
}

#[derive(Default)]
struct ReceiverState {
    channel: Option<ChannelId>,
    generation: u64,
    // Only identity metadata survives /transcribe stop; PCM is never cached here.
    speakers: HashMap<u32, Speaker>,
    synced_token: Option<String>,
}

#[derive(Clone)]
struct DiscordVoiceReceiver {
    guild_id: SerenityGuildId,
    router: Arc<VoiceRouter>,
    http: Arc<Http>,
    cache: Arc<Cache>,
    state: Arc<Mutex<ReceiverState>>,
}

impl DiscordVoiceReceiver {
    fn set_channel(&self, channel: Option<ChannelId>) {
        let mut state = self.state.lock().expect("receiver poisoned");
        if state.channel != channel {
            self.clear(&mut state);
            state.channel = channel;
        }
    }

    fn clear(&self, state: &mut ReceiverState) {
        for speaker in state.speakers.values() {
            self.router
                .disconnect_user(GuildId(self.guild_id.get()), speaker.user_id);
        }
        state.speakers.clear();
        state.synced_token = None;
        state.generation = state.generation.wrapping_add(1);
    }

    fn forget_user(&self, state: &mut ReceiverState, user_id: UserId) {
        state
            .speakers
            .retain(|_, speaker| speaker.user_id != user_id);
        self.router
            .disconnect_user(GuildId(self.guild_id.get()), user_id);
    }

    fn remember(&self, state: &mut ReceiverState, ssrc: u32, speaker: Speaker) {
        if state.speakers.get(&ssrc) == Some(&speaker) {
            return;
        }
        if let Some(previous) = state.speakers.get(&ssrc) {
            if previous.user_id != speaker.user_id {
                self.router
                    .disconnect_user(GuildId(self.guild_id.get()), previous.user_id);
            }
        }
        if state
            .speakers
            .iter()
            .any(|(known_ssrc, known)| *known_ssrc != ssrc && known.user_id == speaker.user_id)
        {
            self.router
                .disconnect_user(GuildId(self.guild_id.get()), speaker.user_id);
        }
        state
            .speakers
            .retain(|_, known| known.user_id != speaker.user_id);
        state.speakers.insert(ssrc, speaker);
        state.synced_token = None;
    }

    fn voice_tick(
        &self,
        speaking: impl IntoIterator<Item = (u32, Vec<i16>)>,
        silent: impl IntoIterator<Item = u32>,
    ) {
        let guild_id = GuildId(self.guild_id.get());
        let Some(token) = self.router.web_token(guild_id) else {
            return;
        };
        let mut state = self.state.lock().expect("receiver poisoned");
        let Some(channel) = state.channel else {
            return;
        };
        if self.router.voice_channel(guild_id) != Some(channel.get())
            || !self.cache.guild(self.guild_id).is_some_and(|guild| {
                !guild.unavailable()
                    && guild
                        .voice_states
                        .get(&self.cache.current_user().id)
                        .and_then(|voice| voice.channel_id)
                        == Some(channel)
            })
        {
            return;
        }
        // Discord need not repeat SSRC identities when transcription is started
        // on an existing TTS call. Replay them once for each session or change.
        if state.synced_token.as_deref() != Some(&token) {
            for (ssrc, speaker) in &state.speakers {
                self.router.speaking_state_for_session(
                    guild_id,
                    Some(&token),
                    *ssrc,
                    speaker.user_id,
                    speaker.name.clone(),
                    speaker.avatar_url.clone(),
                );
            }
            state.synced_token = Some(token.clone());
        }
        self.router
            .voice_tick_for_session(guild_id, Some(&token), speaking, silent);
    }
}

#[async_trait]
impl VoiceEventHandler for DiscordVoiceReceiver {
    async fn act(&self, event: &EventContext<'_>) -> Option<Event> {
        match event {
            EventContext::SpeakingStateUpdate(speaking) => {
                let user_id = SerenityUserId::new(speaking.user_id?.0);
                let (channel, generation) = {
                    let state = self.state.lock().expect("receiver poisoned");
                    (state.channel?, state.generation)
                };
                let cached = self
                    .cache
                    .guild(self.guild_id)
                    .and_then(|guild| guild.members.get(&user_id).cloned());
                let member = match cached {
                    Some(member) => Ok(member),
                    None => self.guild_id.member(&self.http, user_id).await,
                };
                let (name, avatar_url) = match member {
                    Ok(member) if member.user.bot() => return None,
                    Ok(member) => (member.display_name().to_string(), Some(member.face())),
                    Err(_) => (user_id.to_string(), None),
                };
                let mut state = self.state.lock().expect("receiver poisoned");
                if state.generation != generation || state.channel != Some(channel) {
                    return None;
                }
                // A member lookup can finish after a move or departure.
                if !self.cache.guild(self.guild_id).is_some_and(|guild| {
                    guild
                        .voice_states
                        .get(&user_id)
                        .and_then(|voice| voice.channel_id)
                        == Some(channel)
                }) {
                    return None;
                }
                self.remember(
                    &mut state,
                    speaking.ssrc,
                    Speaker {
                        user_id: UserId(user_id.get()),
                        name,
                        avatar_url,
                    },
                );
            }
            EventContext::VoiceTick(tick) => self.voice_tick(
                tick.speaking.iter().filter_map(|(ssrc, data)| {
                    data.decoded_voice.as_ref().map(|pcm| (*ssrc, pcm.clone()))
                }),
                tick.silent.iter().copied(),
            ),
            EventContext::ClientDisconnect(disconnect) => {
                let mut state = self.state.lock().expect("receiver poisoned");
                self.forget_user(&mut state, UserId(disconnect.user_id.0));
            }
            EventContext::DriverDisconnect(_) => {
                self.clear(&mut self.state.lock().expect("receiver poisoned"));
            }
            _ => {}
        }
        None
    }
}
