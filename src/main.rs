use std::{collections::HashMap, env, sync::Arc};

use ncb_tts_r2::{
    config::Config,
    data::UserData,
    database::database::Database,
    errors::{NCBError, Result},
    event_handler::Handler,
    trace::init_tracing_subscriber,
    tts::{gcp_tts::gcp_tts::GCPTTS, tts::TTS, voicevox::voicevox::VOICEVOX},
};
use serenity::prelude::{Client, GatewayIntents, RwLock, Token};
use tracing::info;

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
    let mut config = load_config()?;
    config.configure_features()?;
    let _guard = init_tracing_subscriber(&config.otel_http_url);

    #[cfg(not(feature = "transcription"))]
    let manager = songbird::Songbird::serenity();
    #[cfg(feature = "transcription")]
    let manager = ncb_tts_r2::transcription::voice_manager(&config);
    let shutdown = tokio_util::sync::CancellationToken::new();
    #[cfg(feature = "transcription")]
    #[allow(unused_mut)]
    let (transcription, mut result_events) = if config.transcription.enabled {
        let (service, events) =
            ncb_tts_r2::transcription::Transcription::new(&config, shutdown.clone());
        (Some(service), Some(events))
    } else {
        (None, None)
    };
    #[cfg(feature = "web-ui")]
    let web_config = if config.web.enabled {
        Some(
            ncb_tts_r2::transcription::web::WebConfig::new(&config.web)
                .map_err(|e| NCBError::config(e.to_string()))?,
        )
    } else {
        None
    };

    let tts = GCPTTS::new("./credentials.json".to_string()).await?;
    let voicevox = VOICEVOX::new(config.voicevox_key, config.voicevox_original_api_url)?;
    let database_client = Database::new_with_url(config.redis_url).await?;

    let user_data = UserData {
        #[cfg(feature = "transcription")]
        transcription,
        songbird: Arc::clone(&manager),
        tts_data: Arc::new(RwLock::new(HashMap::default())),
        tts_client: Arc::new(TTS::new(voicevox, tts)),
        database: Arc::new(database_client),
        monitor_started: std::sync::atomic::AtomicBool::new(false),
        shutdown,
        setup_locks: std::sync::Mutex::new(HashMap::new()),
    };

    let token: Token = config
        .token
        .parse()
        .map_err(|_| NCBError::config("Invalid Discord token"))?;

    let user_data = Arc::new(user_data);
    let mut client = Client::builder(token, GatewayIntents::all())
        .event_handler(Arc::new(Handler))
        .voice_manager(manager)
        .data(user_data.clone() as _)
        .await?;

    #[cfg(feature = "web-ui")]
    if let Some(config) = web_config {
        ncb_tts_r2::transcription::web::start(
            config,
            user_data
                .transcription
                .as_ref()
                .expect("enabled transcription")
                .router
                .clone(),
            client.cache.clone(),
            result_events.take().expect("enabled transcription events"),
            user_data.shutdown.clone(),
        )
        .await
        .map_err(|e| NCBError::config(e.to_string()))?;
    }
    #[cfg(feature = "transcription")]
    drop(result_events);

    info!("Bot initialized.");
    let stop_discord = client.shard_manager.get_shutdown_trigger();
    let result = tokio::select! {
        result = client.start() => result,
        _ = shutdown_signal() => Ok(()),
    };
    user_data.shutdown.cancel();
    stop_discord();
    for session in user_data.tts_data.read().await.values() {
        session.cancel();
    }
    result?;
    Ok(())
}

fn load_config() -> Result<Config> {
    if let Ok(config_str) = std::fs::read_to_string("./config.toml") {
        return toml::from_str::<Config>(&config_str).map_err(NCBError::Toml);
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
        transcription: Default::default(),
        web: Default::default(),
    })
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
