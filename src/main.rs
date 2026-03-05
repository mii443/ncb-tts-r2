mod commands;
mod config;
mod connection_monitor;
mod data;
mod database;
mod errors;
mod event_handler;
mod events;
mod implement;
mod interactions;
mod stream_input;
mod trace;
mod tts;
mod utils;

use std::{collections::HashMap, env, sync::Arc};

use config::Config;
use data::UserData;
use database::database::Database;
use errors::{NCBError, Result};
use event_handler::Handler;
use serenity::prelude::{Client, GatewayIntents, RwLock, Token};
use trace::init_tracing_subscriber;
use tracing::info;
use tts::{
    gcp_tts::gcp_tts::GCPTTS, toriel::toriel::TorielTTS, tts::TTS, voicevox::voicevox::VOICEVOX,
};

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls CryptoProvider");

    if let Err(e) = run().await {
        eprintln!("Application error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let config = load_config()?;
    let _guard = init_tracing_subscriber(&config.otel_http_url);

    let manager = songbird::Songbird::serenity();

    let tts = GCPTTS::new("./credentials.json".to_string())
        .await
        .map_err(|e| NCBError::GCPAuth(e))?;
    let voicevox = VOICEVOX::new(config.voicevox_key, config.voicevox_original_api_url);
    let toriel = TorielTTS::new();
    let database_client = Database::new_with_url(config.redis_url).await?;

    let user_data = UserData {
        songbird: Arc::clone(&manager),
        tts_data: Arc::new(RwLock::new(HashMap::default())),
        tts_client: Arc::new(TTS::new(voicevox, tts, toriel)),
        database: Arc::new(database_client),
    };

    let token: Token = config.token.parse().map_err(|_| NCBError::config("Invalid Discord token"))?;

    let mut client = Client::builder(token, GatewayIntents::all())
        .event_handler(Arc::new(Handler))
        .voice_manager(manager)
        .data(Arc::new(user_data) as _)
        .await?;

    info!("Bot initialized.");
    client.start().await?;
    Ok(())
}

fn load_config() -> Result<Config> {
    if let Ok(config_str) = std::fs::read_to_string("./config.toml") {
        return toml::from_str::<Config>(&config_str).map_err(|e| NCBError::Toml(e));
    }

    let token = env::var("NCB_TOKEN").map_err(|_| NCBError::missing_env_var("NCB_TOKEN"))?;
    let application_id_str =
        env::var("NCB_APP_ID").map_err(|_| NCBError::missing_env_var("NCB_APP_ID"))?;
    let prefix = env::var("NCB_PREFIX").map_err(|_| NCBError::missing_env_var("NCB_PREFIX"))?;
    let redis_url =
        env::var("NCB_REDIS_URL").map_err(|_| NCBError::missing_env_var("NCB_REDIS_URL"))?;

    let application_id = application_id_str
        .parse::<u64>()
        .map_err(|_| NCBError::config(format!("Invalid application ID: {}", application_id_str)))?;

    let voicevox_key = env::var("NCB_VOICEVOX_KEY").ok();
    let voicevox_original_api_url = env::var("NCB_VOICEVOX_ORIGINAL_API_URL").ok();
    let otel_http_url = env::var("NCB_OTEL_HTTP_URL").ok();

    Ok(Config {
        token,
        application_id,
        prefix,
        redis_url,
        voicevox_key,
        voicevox_original_api_url,
        otel_http_url,
    })
}
