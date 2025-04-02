use serenity::{
    all::{
        CommandInteraction, CreateInteractionResponse, CreateInteractionResponseMessage,
        MessageFlags
    },
    model::prelude::UserId,
    prelude::Context,
};

use crate::data::TTSData;

pub async fn skip_command(
    ctx: &Context,
    command: &CommandInteraction,
) -> Result<(), Box<dyn std::error::Error>> {
    if command.guild_id.is_none() {
        command
            .create_response(&ctx.http, 
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("このコマンドはサーバーでのみ使用可能です．")
                        .ephemeral(true)
                ))
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
            .create_response(&ctx.http, 
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content("ボイスチャンネルに参加してから実行してください．")
                        .ephemeral(true)
                ))
            .await?;
        return Ok(());
    }

    let storage_lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<TTSData>()
            .expect("Cannot get TTSStorage")
            .clone()
    };

    {
        let mut storage = storage_lock.write().await;
        if !storage.contains_key(&guild.id) {
            command
                .create_response(&ctx.http, 
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("読み上げしていません")
                            .ephemeral(true)
                    ))
                .await?;
            return Ok(());
        }

        storage.get_mut(&guild.id).unwrap().skip(ctx).await;
    }

    command
        .create_response(&ctx.http, 
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("スキップしました")
            ))
        .await?;

    Ok(())
}