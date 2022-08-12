use serenity::{prelude::Context, model::prelude::{application_command::ApplicationCommandInteraction, InteractionApplicationCommandCallbackDataFlags, UserId}};

use crate::{data::TTSData, tts::instance::TTSInstance};

pub async fn setup_command(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<(), Box<dyn std::error::Error>> {
    if let None = command.guild_id {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("このコマンドはサーバーでのみ使用可能です．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }

    let guild = command.guild_id.unwrap().to_guild_cached(&ctx.cache).await;
    if let None = guild {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("ギルドキャッシュを取得できませんでした．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }
    let guild = guild.unwrap();

    let channel_id = guild
        .voice_states
        .get(&UserId(command.user.id.0))
        .and_then(|state| state.channel_id);

    if let None = channel_id {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("ボイスチャンネルに参加してから実行してください．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }

    let channel_id = channel_id.unwrap();

    let manager = songbird::get(ctx).await.expect("Cannot get songbird client.").clone();

    let storage_lock = {
        let data_read = ctx.data.read().await;
        data_read.get::<TTSData>().expect("Cannot get TTSStorage").clone()
    };

    {
        let mut storage = storage_lock.write().await;
        if storage.contains_key(&guild.id) {
            command.create_interaction_response(&ctx.http, |f| {
                f.interaction_response_data(|d| {
                    d.content("すでにセットアップしています．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                })
            }).await?;
            return Ok(());
        }

        storage.insert(guild.id, TTSInstance {
            before_message: None,
            guild: guild.id,
            text_channel: command.channel_id,
            voice_channel: channel_id
        });
    }

    let _handler = manager.join(guild.id.0, channel_id.0).await;

    command.create_interaction_response(&ctx.http, |f| {
        f.interaction_response_data(|d| {
            d.content("セットアップ完了").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
        })
    }).await?;

    Ok(())
}