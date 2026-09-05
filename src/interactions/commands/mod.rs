use crate::errors::{NCBError, Result};
use serenity::{all::CommandInteraction, prelude::Context};

pub async fn handle_command(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    match command.data.name.as_str() {
        "setup" => crate::commands::setup::setup_command(ctx, command).await,
        "stop" => crate::commands::stop::stop_command(ctx, command).await,
        "skip" => crate::commands::skip::skip_command(ctx, command).await,
        "config" => crate::commands::config::config_command(ctx, command)
            .await
            .map_err(|error| NCBError::config(format!("Config command failed: {error}"))),
        _ => Ok(()),
    }
}
