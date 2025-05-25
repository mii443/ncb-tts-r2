use serenity::{
    all::{Command, CommandOptionType, CreateCommand, CreateCommandOption},
    model::prelude::Ready,
    prelude::Context,
};
use tracing::info;

use crate::data::{DatabaseClientData, TTSData};

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
                // Try to reconnect to voice channel
                match instance.reconnect(ctx).await {
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
