mod commands;
mod config;
mod data;
mod database;
mod event_handler;
mod events;
mod implement;
mod tts;

use std::{collections::HashMap, env, sync::Arc};

use config::Config;
use data::{DatabaseClientData, TTSClientData, TTSData};
use database::database::Database;
use event_handler::Handler;
use serenity::{
    client::Client,
    framework::StandardFramework,
    futures::lock::Mutex,
    prelude::{GatewayIntents, RwLock},
};
use tts::{gcp_tts::gcp_tts::TTS, voicevox::voicevox::VOICEVOX};

use songbird::SerenityInit;

/// Create discord client
///
/// Example:
/// ```rust
/// let client = create_client("!", "BOT_TOKEN", 123456789123456789).await;
///
/// client.start().await;
/// ```
async fn create_client(prefix: &str, token: &str, id: u64) -> Result<Client, serenity::Error> {
    let framework = StandardFramework::new().configure(|c| c.with_whitespace(true).prefix(prefix));

    Client::builder(token, GatewayIntents::all())
        .event_handler(Handler)
        .application_id(id)
        .framework(framework)
        .register_songbird()
        .await
}

#[tokio::main]
async fn main() {
    // Load config
    let config = {
        let config = std::fs::read_to_string("./config.toml");
        if let Ok(config) = config {
            toml::from_str::<Config>(&config).expect("Cannot load config file.")
        } else {
            let token = env::var("NCB_TOKEN").unwrap();
            let application_id = env::var("NCB_APP_ID").unwrap();
            let prefix = env::var("NCB_PREFIX").unwrap();
            let redis_url = env::var("NCB_REDIS_URL").unwrap();
            let voicevox_key = env::var("NCB_VOICEVOX_KEY").unwrap();

            Config {
                token,
                application_id: u64::from_str_radix(&application_id, 10).unwrap(),
                prefix,
                redis_url,
                voicevox_key,
            }
        }
    };

    // Create discord client
    let mut client = create_client(&config.prefix, &config.token, config.application_id)
        .await
        .expect("Err creating client");

    // Create GCP TTS client
    let tts = match TTS::new("./credentials.json".to_string()).await {
        Ok(tts) => tts,
        Err(err) => panic!("GCP init error: {}", err),
    };

    let voicevox = VOICEVOX::new(config.voicevox_key);

    let database_client = {
        let redis_client = redis::Client::open(config.redis_url).unwrap();
        Database::new(redis_client)
    };

    // Create TTS storage
    {
        let mut data = client.data.write().await;
        data.insert::<TTSData>(Arc::new(RwLock::new(HashMap::default())));
        data.insert::<TTSClientData>(Arc::new(Mutex::new((tts, voicevox))));
        data.insert::<DatabaseClientData>(Arc::new(Mutex::new(database_client)));
    }

    // Run client
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
