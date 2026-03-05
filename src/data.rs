use crate::{database::database::Database, tts::tts::TTS};
use serenity::{
    model::id::GuildId,
    prelude::RwLock,
};

use crate::tts::instance::TTSInstance;
use std::{collections::HashMap, sync::Arc};

pub struct UserData {
    pub songbird: Arc<songbird::Songbird>,
    pub tts_data: Arc<RwLock<HashMap<GuildId, TTSInstance>>>,
    pub tts_client: Arc<TTS>,
    pub database: Arc<Database>,
}
