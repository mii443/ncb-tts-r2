use serenity::{
    model::prelude::{
        interaction::{application_command::ApplicationCommandInteraction, MessageFlags},
        UserId,
    },
    prelude::Context,
};

use crate::{data::TTSData, tts::instance::TTSInstance};

pub async fn setup_command(
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

    let channel_id = channel_id.unwrap();

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
        if storage.contains_key(&guild.id) {
            command
                .create_interaction_response(&ctx.http, |f| {
                    f.interaction_response_data(|d| {
                        d.content("すでにセットアップしています．")
                            .flags(MessageFlags::EPHEMERAL)
                    })
                })
                .await?;
            return Ok(());
        }

        let text_channel_id = {
            if let Some(mode) = command.data.options.get(0) {
                let mode = mode.clone();
                let value = mode.value.unwrap();
                let value = value.as_str().unwrap();
                match value {
                    "TEXT_CHANNEL" => command.channel_id,
                    "NEW_THREAD" => {
                        let message = command
                            .channel_id
                            .send_message(&ctx.http, |f| f.content("TTS thread"))
                            .await
                            .unwrap();
                        command
                            .channel_id
                            .create_public_thread(&ctx.http, message, |f| {
                                f.name("TTS").auto_archive_duration(60)
                            })
                            .await
                            .unwrap()
                            .id
                    }
                    "VOICE_CHANNEL" => channel_id,
                    _ => channel_id,
                }
            } else {
                channel_id
            }
        };

        storage.insert(
            guild.id,
            TTSInstance {
                before_message: None,
                guild: guild.id,
                text_channel: text_channel_id,
                voice_channel: channel_id,
            },
        );

        text_channel_id
    };

    let _handler = manager.join(guild.id.0, channel_id.0).await;
    command
        .create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content(format!("TTS Channel: <#{}>{}", text_channel_id, if text_channel_id == channel_id { "\nボイスチャンネルを右クリックし `チャットを開く` を押して開くことが出来ます。" } else { "" }))
            })
        })
        .await?;

    text_channel_id.send_message(&ctx.http, |f| f.embed(|e| e.title("読み上げ (Serenity)")
                    .field("クレジット", "```\n四国めたん　　ずんだもん\n春日部つむぎ　雨晴はう\n波音リツ　　　玄野武宏\n白上虎太郎　　青山龍星\n冥鳴ひまり　　九州そら\nモチノ・キョウコ```", false)
                    .field("設定コマンド", "`/config`", false)
                    .field("フィードバック", "https://feedback.mii.codes/", false)
    )).await?;

    Ok(())
}
