use std::{sync::Arc, collections::HashMap};

use config::Config;
use data::{TTSData, TTSClientData};
use event_handler::Handler;
use tts::gcp_tts::gcp_tts::TTS;
use serenity::{
    client::{Client, bridge::gateway::GatewayIntents},
    framework::StandardFramework, prelude::RwLock, futures::lock::Mutex
};

use songbird::SerenityInit;

mod config;
mod event_handler;
mod tts;
mod implement;
mod data;

/// Create discord client
///
/// Example:
/// ```rust
/// let client = create_client("!", "BOT_TOKEN", 123456789123456789).await;
///
/// client.start().await;
/// ```
async fn create_client(prefix: &str, token: &str, id: u64) -> Result<Client, serenity::Error> {
    let framework = StandardFramework::new()
    .configure(|c| c
        .with_whitespace(true)
        .prefix(prefix));

    Client::builder(token)
        .event_handler(Handler)
        .application_id(id)
        .framework(framework)
        .intents(GatewayIntents::all())
        .register_songbird()
        .await
}

#[tokio::main]
async fn main() {
    // Load config
    let config = std::fs::read_to_string("./config.toml").expect("Cannot read config file.");
    let config: Config = toml::from_str(&config).expect("Cannot load config file.");

    // Create discord client
    let mut client = create_client(&config.prefix, &config.token, config.application_id).await.expect("Err creating client");

    // Create TTS client
    let tts = match TTS::new("./credentials.json".to_string()).await {
        Ok(tts) => tts,
        Err(err) => panic!("{}", err)
    };

    // Create TTS storage
    {
        let mut data = client.data.write().await;
        data.insert::<TTSData>(Arc::new(RwLock::new(HashMap::default())));
        data.insert::<TTSClientData>(Arc::new(Mutex::new(tts)));
    }

    // Run client
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
