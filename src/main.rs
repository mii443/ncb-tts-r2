mod commands;
mod config;
mod connection_monitor;
mod data;
mod database;
mod errors;
mod event_handler;
mod events;
mod implement;
mod stream_input;
mod trace;
mod tts;
mod utils;

use std::{collections::HashMap, env, sync::Arc};

use config::Config;
use data::{DatabaseClientData, TTSClientData, TTSData};
use database::database::Database;
use errors::{NCBError, Result};
use event_handler::Handler;
#[allow(deprecated)]
use serenity::{
    all::{standard::Configuration, ApplicationId},
    client::Client,
    framework::StandardFramework,
    prelude::{GatewayIntents, RwLock},
};
use trace::init_tracing_subscriber;
use tracing::info;
use tts::{gcp_tts::gcp_tts::GCPTTS, tts::TTS, voicevox::voicevox::VOICEVOX};

use songbird::SerenityInit;

/// Create discord client
///
/// Example:
/// ```rust
/// let client = create_client("!", "BOT_TOKEN", 123456789123456789).await;
///
/// client.start().await;
/// ```
#[allow(deprecated)]
async fn create_client(prefix: &str, token: &str, id: u64) -> Result<Client> {
    let framework = StandardFramework::new();
    framework.configure(Configuration::new().with_whitespace(true).prefix(prefix));

    Ok(Client::builder(token, GatewayIntents::all())
        .event_handler(Handler)
        .application_id(ApplicationId::new(id))
        .framework(framework)
        .register_songbird()
        .await?)
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Application error: {}", e);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    // Load config
    let config = load_config()?;

    let _guard = init_tracing_subscriber(&config.otel_http_url);

    // Create discord client
    let mut client = create_client(&config.prefix, &config.token, config.application_id)
        .await?;

    // Create GCP TTS client
    let tts = GCPTTS::new("./credentials.json".to_string())
        .await
        .map_err(|e| NCBError::GCPAuth(e))?;

    let voicevox = VOICEVOX::new(config.voicevox_key, config.voicevox_original_api_url);

    let database_client = Database::new_with_url(config.redis_url).await?;

    // Create TTS storage
    {
        let mut data = client.data.write().await;
        data.insert::<TTSData>(Arc::new(RwLock::new(HashMap::default())));
        data.insert::<TTSClientData>(Arc::new(TTS::new(voicevox, tts)));
        data.insert::<DatabaseClientData>(Arc::new(database_client.clone()));
    }

    info!("Bot initialized.");

    // Run client
    client.start().await?;
    
    Ok(())
}

/// Load configuration from file or environment variables
fn load_config() -> Result<Config> {
    // Try to load from config file first
    if let Ok(config_str) = std::fs::read_to_string("./config.toml") {
        return toml::from_str::<Config>(&config_str)
            .map_err(|e| NCBError::Toml(e));
    }
    
    // Fall back to environment variables
    let token = env::var("NCB_TOKEN")
        .map_err(|_| NCBError::missing_env_var("NCB_TOKEN"))?;
    let application_id_str = env::var("NCB_APP_ID")
        .map_err(|_| NCBError::missing_env_var("NCB_APP_ID"))?;
    let prefix = env::var("NCB_PREFIX")
        .map_err(|_| NCBError::missing_env_var("NCB_PREFIX"))?;
    let redis_url = env::var("NCB_REDIS_URL")
        .map_err(|_| NCBError::missing_env_var("NCB_REDIS_URL"))?;
    
    let application_id = application_id_str.parse::<u64>()
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
