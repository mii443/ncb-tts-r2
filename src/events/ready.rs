use serenity::{
    all::{Command, CommandOptionType, CreateCommand, CreateCommandOption},
    model::prelude::Ready,
    prelude::Context,
};
use tracing::info;

use crate::{
    connection_monitor::ConnectionMonitor,
    data::{DatabaseClientData, TTSData},
};

#[tracing::instrument]
pub async fn ready(ctx: Context, ready: Ready) {
    info!("{} is connected!", ready.user.name);

    Command::set_global_commands(
        &ctx.http,
        vec![
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
        ],
    )
    .await
    .unwrap();

    // Restore TTS instances from database
    restore_tts_instances(&ctx).await;

    // Start connection monitor
    ConnectionMonitor::start(ctx.clone());
}

/// Restore TTS instances from database and reconnect to voice channels
async fn restore_tts_instances(ctx: &Context) {
    info!("Restoring TTS instances from database...");

    let data = ctx.data.read().await;
    let database = data
        .get::<DatabaseClientData>()
        .expect("Cannot get DatabaseClientData")
        .clone();
    let tts_data = data.get::<TTSData>().unwrap().clone();
    drop(data);

    match database.get_all_tts_instances().await {
        Ok(instances) => {
            let mut restored_count = 0;
            let mut failed_count = 0;

            for (guild_id, instance) in instances {
                // Check if there are users in the voice channel before reconnecting
                let should_reconnect = match guild_id.channels(&ctx.http).await {
                    Ok(channels) => {
                        if let Some(channel) = channels.get(&instance.voice_channel) {
                            match channel.members(&ctx.cache) {
                                Ok(members) => {
                                    let user_count =
                                        members.iter().filter(|member| !member.user.bot).count();
                                    user_count > 0
                                }
                                Err(_) => {
                                    // If we can't get members, assume there are no users
                                    tracing::warn!(
                                        "Failed to get members for voice channel {} in guild {}",
                                        instance.voice_channel,
                                        guild_id
                                    );
                                    false
                                }
                            }
                        } else {
                            // Channel doesn't exist anymore
                            tracing::warn!(
                                "Voice channel {} no longer exists in guild {}",
                                instance.voice_channel,
                                guild_id
                            );
                            false
                        }
                    }
                    Err(_) => {
                        // If we can't get channels, assume reconnection should not happen
                        tracing::warn!("Failed to get channels for guild {}", guild_id);
                        false
                    }
                };

                if !should_reconnect {
                    // Remove instance from database as the channel is empty or doesn't exist
                    failed_count += 1;
                    tracing::info!("Skipping reconnection for guild {} - no users in voice channel or channel doesn't exist", guild_id);

                    if let Err(db_err) = database.remove_tts_instance(guild_id).await {
                        tracing::error!(
                            "Failed to remove empty TTS instance from database: {}",
                            db_err
                        );
                    }
                    continue;
                }

                // Try to reconnect to voice channel
                match instance.reconnect(ctx, true).await {
                    Ok(_) => {
                        // Add to in-memory storage
                        let mut tts_data = tts_data.write().await;
                        tts_data.insert(guild_id, instance);
                        drop(tts_data);

                        restored_count += 1;
                        info!("Restored TTS instance for guild {}", guild_id);
                    }
                    Err(e) => {
                        failed_count += 1;
                        tracing::warn!(
                            "Failed to restore TTS instance for guild {}: {}",
                            guild_id,
                            e
                        );

                        // Remove failed instance from database
                        if let Err(db_err) = database.remove_tts_instance(guild_id).await {
                            tracing::error!(
                                "Failed to remove invalid TTS instance from database: {}",
                                db_err
                            );
                        }
                    }
                }
            }

            info!(
                "TTS restoration complete: {} restored, {} failed",
                restored_count, failed_count
            );
        }
        Err(e) => {
            tracing::error!("Failed to load TTS instances from database: {}", e);
        }
    }
}
