//! Configuration toggle button handlers
//!
//! Handles buttons that toggle boolean settings (voice announce, read username)

use crate::{errors::Result, interactions::utils};
use serenity::{all::ComponentInteraction, prelude::Context};

/// Handle voice state announce toggle button
pub async fn handle_voice_state_announce_toggle(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    toggle_boolean_setting(
        ctx,
        interaction,
        |config| &mut config.voice_state_announce,
        |state| {
            format!(
                "入退出アナウンス通知を{}へ切り替えました。",
                if state { "`有効`" } else { "`無効`" }
            )
        },
    )
    .await
}

/// Handle read username toggle button
pub async fn handle_read_username_toggle(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    toggle_boolean_setting(
        ctx,
        interaction,
        |config| &mut config.read_username,
        |state| {
            format!(
                "ユーザー名読み上げを{}へ切り替えました。",
                if state { "`有効`" } else { "`無効`" }
            )
        },
    )
    .await
}

/// Shared helper function to toggle boolean settings
/// This eliminates ~80 lines of duplicated code
async fn toggle_boolean_setting<F, G>(
    ctx: &Context,
    interaction: &ComponentInteraction,
    field_updater: F,
    success_message: G,
) -> Result<()>
where
    F: FnOnce(&mut crate::database::server_config::ServerConfig) -> &mut Option<bool>,
    G: Fn(bool) -> String,
{
    let guild_id = utils::extract_guild_id(interaction)?;
    let mut config = utils::get_server_config(ctx, guild_id).await?;

    // Toggle the field
    let field = field_updater(&mut config);
    *field = Some(!field.unwrap_or(true));
    let new_state = field.unwrap_or(true);

    // Save config
    utils::set_server_config(ctx, guild_id, config).await?;

    // Send response
    utils::update_interaction_message(ctx, interaction, success_message(new_state)).await?;

    Ok(())
}
