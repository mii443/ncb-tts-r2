use crate::{
    database::database::Database,
    tts::tts::TTS,
};
use serenity::{
    futures::lock::Mutex,
    model::id::GuildId,
    prelude::{RwLock, TypeMapKey},
};

use crate::tts::instance::TTSInstance;
use std::{collections::HashMap, sync::Arc};

/// TTSInstance data
pub struct TTSData;

impl TypeMapKey for TTSData {
    type Value = Arc<RwLock<HashMap<GuildId, TTSInstance>>>;
}

/// TTS client data
pub struct TTSClientData;

impl TypeMapKey for TTSClientData {
    type Value = Arc<TTS>;
}

/// Database client data
pub struct DatabaseClientData;

impl TypeMapKey for DatabaseClientData {
    type Value = Arc<Mutex<Database>>;
}
