use crate::{tts::gcp_tts::gcp_tts::TTS, database::database::Database};
use serenity::{prelude::{TypeMapKey, RwLock}, model::id::GuildId, futures::lock::Mutex};

use crate::tts::instance::TTSInstance;
use std::{sync::Arc, collections::HashMap};

/// TTSInstance data
pub struct TTSData;

impl TypeMapKey for TTSData {
    type Value = Arc<RwLock<HashMap<GuildId, TTSInstance>>>;
}

/// TTS client data
pub struct TTSClientData;

impl TypeMapKey for TTSClientData {
    type Value = Arc<Mutex<TTS>>;
}

/// Database client data
pub struct DatabaseClientData;

impl TypeMapKey for DatabaseClientData {
    type Value = Arc<Mutex<Database>>;
}
