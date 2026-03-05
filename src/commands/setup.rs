use serenity::{
    all::{
        AutoArchiveDuration, ChannelId, CommandInteraction, CreateEmbed, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateMessage, CreateThread,
    },
    model::prelude::UserId,
    prelude::Context,
};
use tracing::info;

use crate::{
    data::UserData,
    tts::instance::TTSInstance,
};

#[tracing::instrument(skip_all)]
pub async fn setup_command(
    ctx: &Context,
    command: &CommandInteraction,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Received event");
    
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

    info!("Fetching guild cache");
    let guild_id = command.guild_id.unwrap();
    let guild = guild_id.to_guild_cached(&ctx.cache).unwrap().clone();

    let channel_id = guild
        .voice_states
        .get(&UserId::from(command.user.id.get()))
        .and_then(|voice_state| voice_state.channel_id);

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

    let channel_id = channel_id.unwrap();

    let data = ctx.data::<UserData>();
    let manager = data.songbird.clone();
    let storage_lock = data.tts_data.clone();

    let cmd_channel_id = ChannelId::new(command.channel_id.get());

    let text_channel_id = {
        let mut storage = storage_lock.write().await;
        if storage.contains_key(&guild.id) {
            command
                .create_response(&ctx.http, 
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("すでにセットアップしています．")
                            .ephemeral(true)
                    ))
                .await?;
            return Ok(());
        }

        let text_channel_ids = {
            if let Some(mode) = command.data.options.get(0) {
                match &mode.value {
                    serenity::all::CommandDataOptionValue::String(value) => {
                        match value.as_str() {
                            "TEXT_CHANNEL" => vec![cmd_channel_id],
                            "NEW_THREAD" => {
                                let thread = cmd_channel_id
                                    .create_thread(&ctx.http, CreateThread::new("TTS").auto_archive_duration(AutoArchiveDuration::OneHour).kind(serenity::all::ChannelType::PublicThread))
                                    .await
                                    .unwrap();
                                vec![ChannelId::new(thread.id.get())]
                            }
                            "VOICE_CHANNEL" => vec![channel_id],
                            _ => if channel_id != cmd_channel_id {
                                vec![cmd_channel_id, channel_id]
                            } else {
                                vec![channel_id]
                            },
                        }
                    },
                    _ => if channel_id != cmd_channel_id {
                        vec![cmd_channel_id, channel_id]
                    } else {
                        vec![channel_id]
                    },
                }
            } else {
                if channel_id != cmd_channel_id {
                    vec![cmd_channel_id, channel_id]
                } else {
                    vec![channel_id]
                }
            }
        };

        let instance = TTSInstance::new(text_channel_ids.clone(), channel_id, guild.id);
        storage.insert(guild.id, instance.clone());

        if let Err(e) = data.database.save_tts_instance(guild.id, &instance).await {
            tracing::error!("Failed to save TTS instance to database: {}", e);
        }

        text_channel_ids[0]
    };

    command
        .create_response(&ctx.http, 
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(format!(
                        "TTS Channel: <#{}>{}", 
                        text_channel_id, 
                        if text_channel_id == channel_id { 
                            "\nボイスチャンネルを右クリックし `チャットを開く` を押して開くことが出来ます。" 
                        } else { 
                            "" 
                        }
                    ))
            ))
        .await?;

    let _handler = manager.join(guild.id, channel_id).await;

    let tts_client = &data.tts_client;
    let voicevox_speakers = tts_client.voicevox_client.get_speakers().await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to get VOICEVOX speakers: {}", e);
            vec!["VOICEVOX API unavailable".to_string()]
        });

    text_channel_id
        .widen().send_message(&ctx.http, CreateMessage::new()
            .embed(
                CreateEmbed::new()
                    .title("読み上げ (Serenity)")
                    .field(
                        "VOICEVOXクレジット",
                        format!("```\n{}\n```", voicevox_speakers.join("\n")),
                        false,
                    )
                    .field("設定コマンド", "`/config`", false)
                    .field("フィードバック", "https://feedback.mii.codes/", false)
            ))
        .await?;

    Ok(())
}
