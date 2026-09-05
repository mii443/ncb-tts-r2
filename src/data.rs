use crate::{database::database::Database, tts::tts::TTS};
use serenity::{model::id::GuildId, prelude::RwLock};

use crate::tts::session::TTSSession;
use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, Mutex, Weak},
};
use tokio_util::sync::CancellationToken;

pub struct UserData {
    pub songbird: Arc<songbird::Songbird>,
    pub tts_data: Arc<RwLock<HashMap<GuildId, Arc<TTSSession>>>>,
    pub tts_client: Arc<TTS>,
    pub database: Arc<Database>,
    pub monitor_started: AtomicBool,
    pub shutdown: CancellationToken,
    pub setup_locks: Mutex<HashMap<GuildId, Weak<tokio::sync::Mutex<()>>>>,
}

impl UserData {
    pub async fn setup_guard(&self, guild: GuildId) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.setup_locks.lock().unwrap();
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(&guild).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                locks.insert(guild, Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }
}
