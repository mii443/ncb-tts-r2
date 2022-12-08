use serenity::{
    model::prelude::{
        interaction::{application_command::ApplicationCommandInteraction, MessageFlags},
        UserId,
    },
    prelude::Context,
};

use crate::data::TTSData;

pub async fn skip_command(
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
                .create_interaction_response(&ctx.http, |f| {
                    f.interaction_response_data(|d| {
                        d.content("読み上げしていません")
                            .flags(MessageFlags::EPHEMERAL)
                    })
                })
                .await?;
            return Ok(());
        }

        storage.get_mut(&guild.id).unwrap().skip(&ctx).await;
    }

    command
        .create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| d.content("スキップしました"))
        })
        .await?;

    Ok(())
}
