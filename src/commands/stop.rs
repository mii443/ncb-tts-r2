use serenity::{
    all::{
        CommandInteraction, CreateInteractionResponse, CreateInteractionResponseMessage, EditThread,
        ThreadId,
    },
    model::prelude::UserId,
    prelude::Context,
};

use crate::data::UserData;

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

    let data = ctx.data::<UserData>();
    let manager = data.songbird.clone();
    let storage_lock = data.tts_data.clone();

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

        let text_channel_id = storage.get(&guild.id).unwrap().text_channels[0];
        storage.remove(&guild.id);

        if let Err(e) = data.database.remove_tts_instance(guild.id).await {
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

    let _ = EditThread::new()
        .archived(true)
        .execute(&ctx.http, ThreadId::new(text_channel_id.get()))
        .await;

    Ok(())
}
