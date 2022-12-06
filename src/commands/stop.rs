use serenity::{
    model::prelude::{
        interaction::{application_command::ApplicationCommandInteraction, MessageFlags},
        UserId,
    },
    prelude::Context,
};

use crate::data::TTSData;

pub async fn stop_command(
    ctx: &Context,
    command: &ApplicationCommandInteraction,
) -> Result<(), Box<dyn std::error::Error>> {
    if let None = command.guild_id {
        command
            .create_interaction_response(&ctx.http, |f| {
                f.interaction_response_data(|d| {
                    d.content("このコマンドはサーバーでのみ使用可能です．")
                        .flags(MessageFlags::EPHEMERAL)
                })
            })
            .await?;
        return Ok(());
    }

    let guild = command.guild_id.unwrap().to_guild_cached(&ctx.cache);
    if let None = guild {
        command
            .create_interaction_response(&ctx.http, |f| {
                f.interaction_response_data(|d| {
                    d.content("ギルドキャッシュを取得できませんでした．")
                        .flags(MessageFlags::EPHEMERAL)
                })
            })
            .await?;
        return Ok(());
    }
    let guild = guild.unwrap();

    let channel_id = guild
        .voice_states
        .get(&UserId(command.user.id.0))
        .and_then(|state| state.channel_id);

    if let None = channel_id {
        command
            .create_interaction_response(&ctx.http, |f| {
                f.interaction_response_data(|d| {
                    d.content("ボイスチャンネルに参加してから実行してください．")
                        .flags(MessageFlags::EPHEMERAL)
                })
            })
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
                .create_interaction_response(&ctx.http, |f| {
                    f.interaction_response_data(|d| {
                        d.content("すでに停止しています")
                            .flags(MessageFlags::EPHEMERAL)
                    })
                })
                .await?;
            return Ok(());
        }

        let text_channel_id = storage.get(&guild.id).unwrap().text_channel;

        storage.remove(&guild.id);

        text_channel_id
    };

    let _handler = manager.remove(guild.id.0).await;

    command
        .create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| d.content("停止しました"))
        })
        .await?;

    let _ = text_channel_id
        .edit_thread(&ctx.http, |f| f.archived(true))
        .await;

    Ok(())
}
