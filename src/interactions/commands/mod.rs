//! Command handlers module
//!
//! This module re-exports existing command handlers from the crate::commands module.
//! Future work: Migrate commands here with improved error handling using crate::errors::Result.

use crate::errors::Result;
use serenity::{all::CommandInteraction, prelude::Context};

/// Handle command interactions
/// Currently delegates to existing command handlers in crate::commands
pub async fn handle_command(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    match command.data.name.as_str() {
        "setup" => {
            crate::commands::setup::setup_command(ctx, command)
                .await
                .map_err(|e| crate::errors::NCBError::config(&format!("Setup command failed: {}", e)))?;
            Ok(())
        }
        "stop" => {
            crate::commands::stop::stop_command(ctx, command)
                .await
                .map_err(|e| crate::errors::NCBError::config(&format!("Stop command failed: {}", e)))?;
            Ok(())
        }
        "config" => {
            crate::commands::config::config_command(ctx, command)
                .await
                .map_err(|e| crate::errors::NCBError::config(&format!("Config command failed: {}", e)))?;
            Ok(())
        }
        "skip" => {
            crate::commands::skip::skip_command(ctx, command)
                .await
                .map_err(|e| crate::errors::NCBError::config(&format!("Skip command failed: {}", e)))?;
            Ok(())
        }
        _ => Ok(()),
    }
}
