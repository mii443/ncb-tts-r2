use crate::{
    data::{DatabaseClientData, TTSClientData, TTSData},
    implement::{
        member_name::ReadName,
        voice_move_state::{VoiceMoveState, VoiceMoveStateTrait},
    },
    tts::{instance::TTSInstance, message::AnnounceMessage},
};
use serenity::{all::{CreateEmbed, CreateMessage, EditThread}, model::voice::VoiceState, prelude::Context};

pub async fn voice_state_update(ctx: Context, old: Option<VoiceState>, new: VoiceState) {
    if new.member.clone().unwrap().user.bot {
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

    let storage_lock = {
        let data_read = ctx.data.read().await;
        data_read
            .get::<TTSData>()
            .expect("Cannot get TTSStorage")
            .clone()
    };

    let config = {
        let data_read = ctx.data.read().await;
        let database = data_read
            .get::<DatabaseClientData>()
            .expect("Cannot get DatabaseClientData")
            .clone();
        let mut database = database.lock().await;
        database
            .get_server_config_or_default(guild_id.get())
            .await
            .unwrap()
            .unwrap()
    };

    {
        let mut storage = storage_lock.write().await;
        if !storage.contains_key(&guild_id) {
            if let Some(new_channel) = new.channel_id {
                if config.autostart_channel_id.unwrap_or(0) == new_channel.get() {
                    let manager = songbird::get(&ctx)
                        .await
                        .expect("Cannot get songbird client.")
                        .clone();
                    storage.insert(
                        guild_id,
                        TTSInstance {
                            before_message: None,
                            guild: guild_id,
                            text_channel: new_channel,
                            voice_channel: new_channel,
                        },
                    );

                    let _handler = manager.join(guild_id, new_channel).await;
                    let tts_client = ctx
                        .data
                        .read()
                        .await
                        .get::<TTSClientData>()
                        .expect("Cannot get TTSClientData")
                        .clone();
                    let voicevox_speakers = tts_client.lock().await.1.get_speakers().await;

                    new_channel
                        .send_message(&ctx.http, 
                    CreateMessage::new()
                                .embed(
                                    CreateEmbed::new()
                                        .title("自動参加 読み上げ（Serenity）")
                                        .field("VOICEVOXクレジット", format!("```\n{}\n```", voicevox_speakers.join("\n")), false)
                                        .field("設定コマンド", "`/config`", false)
                                        .field("フィードバック", "https://feedback.mii.codes/", false))
                        )
                        .await
                        .unwrap();
                }
            }
            return;
        }

        let instance = storage.get_mut(&guild_id).unwrap();

        let voice_move_state = new.move_state(&old, instance.voice_channel);

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

        if voice_move_state == VoiceMoveState::LEAVE {
            let mut del_flag = false;
            for channel in guild_id.channels(&ctx.http).await.unwrap() {
                if channel.0 == instance.voice_channel {
                    del_flag = channel.1.members(&ctx.cache).unwrap().len() <= 1;
                }
            }

            if del_flag {
                let _ = storage
                    .get(&guild_id)
                    .unwrap()
                    .text_channel
                    .edit_thread(&ctx.http, EditThread::new().archived(true))
                    .await;
                storage.remove(&guild_id);

                let manager = songbird::get(&ctx)
                    .await
                    .expect("Cannot get songbird client.")
                    .clone();

                manager.remove(guild_id).await.unwrap();
            }
        }
    }
}
