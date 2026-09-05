use crate::connection_monitor::ConnectionMonitor;
use serenity::{
    all::{Command, CommandOptionType, CreateCommand, CreateCommandOption},
    model::prelude::Ready,
    prelude::Context,
};

#[tracing::instrument(skip_all)]
pub async fn ready(ctx: &Context, ready: &Ready) {
    tracing::info!("{} is connected!", ready.user.name);
    let commands = vec![
        CreateCommand::new("stop").description("Stop tts"),
        CreateCommand::new("setup")
            .description("Setup tts")
            .set_options(vec![CreateCommandOption::new(
                CommandOptionType::String,
                "mode",
                "TTS channel",
            )
            .add_string_choice("Text Channel", "TEXT_CHANNEL")
            .add_string_choice("New Thread", "NEW_THREAD")
            .add_string_choice("Voice Channel", "VOICE_CHANNEL")
            .required(false)]),
        CreateCommand::new("config").description("Config"),
        CreateCommand::new("skip").description("skip tts message"),
    ];
    if let Err(error) = Command::set_global_commands(&ctx.http, &commands).await {
        tracing::error!(error = %error, "Failed to register commands");
    }
    // The monitor retries restoration after Redis/cache failures and starts only once.
    ConnectionMonitor::start(ctx.clone());
}
