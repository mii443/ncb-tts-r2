use futures::{stream, StreamExt};
use serenity::{model::id::GuildId, prelude::Context};
use std::{collections::HashMap, sync::atomic::Ordering, sync::Arc, time::Duration};
use tokio::time::{Instant, MissedTickBehavior};

use crate::{
    data::UserData,
    errors::{constants::CONNECTION_CHECK_INTERVAL_SECS, Result},
    tts::session::{voice_presence, TTSSession, VoicePresence},
};

#[derive(Default)]
pub struct ConnectionMonitor {
    retries: HashMap<GuildId, RetryState>,
    restored: bool,
}

struct RetryState {
    failures: u32,
    next_attempt: Instant,
}

impl RetryState {
    fn failed(previous: Option<Self>) -> Self {
        let failures = previous.map_or(1, |retry| retry.failures.saturating_add(1));
        let seconds = (2u64.saturating_pow(failures.min(6))).min(60);
        Self {
            failures,
            next_attempt: Instant::now() + Duration::from_secs(seconds),
        }
    }
}

impl ConnectionMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(ctx: Context) {
        let data = ctx.data::<UserData>();
        if data.monitor_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let shutdown = data.shutdown.clone();
        tokio::spawn(async move {
            let mut monitor = Self::new();
            let mut interval =
                tokio::time::interval(Duration::from_secs(CONNECTION_CHECK_INTERVAL_SECS));
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    _ = interval.tick() => {},
                }
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    _ = monitor.check_connections(&ctx) => {},
                }
            }
        });
    }

    async fn restore(&mut self, ctx: &Context) -> Result<()> {
        let data = ctx.data::<UserData>();
        let mut failure = None;
        for id in data.database.list_active_instances().await? {
            if id == 0 {
                failure = Some(crate::errors::NCBError::database("Invalid saved guild ID"));
                continue;
            }
            let guild = GuildId::new(id);
            let _guard = data.setup_guard(guild).await;
            if data.tts_data.read().await.contains_key(&guild) {
                continue;
            }
            // Load under the same per-guild guard as stop, so a stale snapshot
            // cannot recreate a session after its saved state has been deleted.
            match data.database.load_tts_instance(guild).await {
                Ok(Some(instance)) => {
                    data.tts_data
                        .write()
                        .await
                        .insert(guild, TTSSession::new(instance, ctx.clone(), true));
                }
                Ok(None) => {}
                Err(error) => failure = Some(error),
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        self.restored = true;
        Ok(())
    }

    async fn check_connections(&mut self, ctx: &Context) {
        if !self.restored {
            if let Err(error) = self.restore(ctx).await {
                tracing::warn!(error = %error, "Cannot restore sessions yet; saved state is retained");
            }
        }
        let sessions: Vec<_> = ctx
            .data::<UserData>()
            .tts_data
            .read()
            .await
            .iter()
            .map(|(&guild, session)| (guild, session.clone()))
            .collect();
        self.retries
            .retain(|guild, _| sessions.iter().any(|(id, _)| id == guild));
        let due = sessions
            .into_iter()
            .filter(|(guild, session)| {
                session.is_stopped()
                    || self
                        .retries
                        .get(guild)
                        .is_none_or(|retry| Instant::now() >= retry.next_attempt)
            })
            .collect::<Vec<_>>();
        let results = stream::iter(due)
            .map(|(guild, session)| async move { (guild, check_session(ctx, session).await) })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        for (guild, result) in results {
            match result {
                Ok(()) => {
                    self.retries.remove(&guild);
                }
                Err(error) => {
                    let retry = RetryState::failed(self.retries.remove(&guild));
                    tracing::warn!(guild_id = %guild, failures = retry.failures, error = %error, "Session recovery deferred; saved state is retained");
                    self.retries.insert(guild, retry);
                }
            }
        }
    }
}

async fn check_session(ctx: &Context, session: Arc<TTSSession>) -> Result<()> {
    if session.is_stopped() {
        return session.stop(ctx).await;
    }
    match voice_presence(ctx, &session.instance) {
        VoicePresence::Unknown => Ok(()),
        VoicePresence::Empty => session.stop(ctx).await,
        VoicePresence::Occupied => session.connect(ctx).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_reconnections_keep_retrying_with_bounded_backoff() {
        let mut state = None;
        for expected in 1..=20 {
            let retry = RetryState::failed(state);
            assert_eq!(retry.failures, expected);
            assert!(retry.next_attempt > Instant::now());
            assert!(retry.next_attempt.duration_since(Instant::now()) <= Duration::from_secs(60));
            state = Some(retry);
        }
    }
}
