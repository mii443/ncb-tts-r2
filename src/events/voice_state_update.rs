use crate::{
    data::UserData,
    implement::{
        member_name::ReadName,
        voice_move_state::{VoiceMoveState, VoiceMoveStateTrait},
    },
    tts::{instance::TTSInstance, message::AnnounceMessage},
};
use serenity::{
    all::{CreateEmbed, CreateMessage, EditThread, ThreadId},
    model::voice::VoiceState,
    prelude::Context,
};

pub async fn voice_state_update(ctx: &Context, old: Option<VoiceState>, new: VoiceState) {
    if new.member.clone().unwrap().user.bot() {
        return;
    }

    if old.is_none() && new.guild_id.is_none() {
        return;
    }

    let guild_id = if let Some(guild_id) = new.guild_id {
        guild_id
    } else {
        old.clone().unwrap().guild_id.unwrap()
    };

    let data = ctx.data::<UserData>();
    let storage_lock = data.tts_data.clone();
    let database = data.database.clone();

    let config = database
        .get_server_config_or_default(guild_id.get())
        .await
        .unwrap()
        .unwrap();

    {
        let mut storage = storage_lock.write().await;
        if !storage.contains_key(&guild_id) {
            if let Some(new_channel) = new.channel_id {
                if config.autostart_channel_id.unwrap_or(0) == new_channel.get() {
                    let manager = data.songbird.clone();

                    let text_channel_ids =
                        if let Some(text_channel_id) = config.autostart_text_channel_id {
                            vec![text_channel_id.into(), new_channel]
                        } else {
                            vec![new_channel]
                        };

                    let instance = TTSInstance::new(text_channel_ids, new_channel, guild_id);
                    storage.insert(guild_id, instance.clone());

                    if let Err(e) = database.save_tts_instance(guild_id, &instance).await {
                        tracing::error!("Failed to save TTS instance to database: {}", e);
                    }

                    let _handler = manager.join(guild_id, new_channel).await;
                    let tts_client = &data.tts_client;
                    let voicevox_speakers = tts_client
                        .voicevox_client
                        .get_speakers()
                        .await
                        .unwrap_or_else(|e| {
                            tracing::error!("Failed to get VOICEVOX speakers: {}", e);
                            vec!["VOICEVOX API unavailable".to_string()]
                        });

                    let embed = CreateEmbed::new()
                        .title("自動参加 読み上げ（Serenity）")
                        .field(
                            "VOICEVOXクレジット",
                            format!("```\n{}\n```", voicevox_speakers.join("\n")),
                            false,
                        )
                        .field("設定コマンド", "`/config`", false)
                        .field("フィードバック", "https://feedback.mii.codes/", false);
                    let msg = CreateMessage::new().embed(embed);
                    new_channel.widen().send_message(&ctx.http, msg).await.unwrap();
                }
            }
            return;
        }

        let instance = storage.get_mut(&guild_id).unwrap();

        let voice_move_state = new.move_state(&old, instance.voice_channel);

        if config.voice_state_announce.unwrap_or(false) {
            let message: Option<String> = match voice_move_state {
                VoiceMoveState::JOIN => Some(format!(
                    "{} さんが通話に参加しました",
                    new.member.unwrap().read_name()
                )),
                VoiceMoveState::LEAVE => Some(format!(
                    "{} さんが通話から退出しました",
                    new.member.unwrap().read_name()
                )),
                _ => None,
            };

            if let Some(message) = message {
                instance.read(AnnounceMessage { message }, &ctx).await;
            }
        }

        if voice_move_state == VoiceMoveState::LEAVE {
            let mut del_flag = false;
            for channel in guild_id.channels(&ctx.http).await.unwrap() {
                if channel.id == instance.voice_channel {
                    let members = channel.members(&ctx.cache).unwrap();
                    let user_count = members.iter().filter(|member| !member.user.bot()).count();

                    del_flag = user_count == 0;
                }
            }

            if del_flag {
                if let Some(&channel_id) = storage.get(&guild_id).unwrap().text_channels.first() {
                    let http = ctx.http.clone();
                    tokio::spawn(async move {
                        let _ = EditThread::new()
                            .archived(true)
                            .execute(&http, ThreadId::new(channel_id.get()))
                            .await;
                    });
                }
                storage.remove(&guild_id);

                if let Err(e) = database.remove_tts_instance(guild_id).await {
                    tracing::error!("Failed to remove TTS instance from database: {}", e);
                }

                let manager = data.songbird.clone();

                manager.remove(guild_id).await.unwrap();
            }
        }
    }
}
