use serenity::{
    all::{
        CommandInteraction, CreateInteractionResponse, CreateInteractionResponseMessage, EditThread,
    },
    model::prelude::UserId,
    prelude::Context,
};

use crate::data::{DatabaseClientData, TTSData};

pub async fn stop_command(
    ctx: &Context,
    command: &CommandInteraction,
) -> Result<(), Box<dyn std::error::Error>> {
    if command.guild_id.is_none() {
        command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("このコマンドはサーバーでのみ使用可能です．")
                        .ephemeral(true),
                ),
            )
            .await?;
        return Ok(());
    }

    let guild_id = command.guild_id.unwrap();
    let guild = guild_id.to_guild_cached(&ctx.cache).unwrap().clone();

    let channel_id = guild
        .voice_states
        .get(&UserId::from(command.user.id.get()))
        .and_then(|state| state.channel_id);

    if channel_id.is_none() {
        command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("ボイスチャンネルに参加してから実行してください．")
                        .ephemeral(true),
                ),
            )
            .await?;
        return Ok(());
    }

    let manager = songbird::get(ctx)
        .await
        .expect("Cannot get songbird client.")
        .clone();

    let storage_lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<TTSData>()
            .expect("Cannot get TTSStorage")
            .clone()
    };

    let text_channel_id = {
        let mut storage = storage_lock.write().await;

        if !storage.contains_key(&guild.id) {
            command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("すでに停止しています")
                            .ephemeral(true),
                    ),
                )
                .await?;
            return Ok(());
        }

        let text_channel_id = storage.get(&guild.id).unwrap().text_channel;
        storage.remove(&guild.id);

        // Remove from database
        let data_read = ctx.data.read().await;
        let database = data_read
            .get::<DatabaseClientData>()
            .expect("Cannot get DatabaseClientData")
            .clone();
        drop(data_read);

        if let Err(e) = database.remove_tts_instance(guild.id).await {
            tracing::error!("Failed to remove TTS instance from database: {}", e);
        }

        text_channel_id
    };

    let _handler = manager.remove(guild.id).await;

    command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content("停止しました"),
            ),
        )
        .await?;

    let _ = text_channel_id
        .edit_thread(&ctx.http, EditThread::new().archived(true))
        .await;

    Ok(())
}
