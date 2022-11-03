use crate::{
    data::TTSData,
    implement::{
        member_name::ReadName,
        voice_move_state::{VoiceMoveState, VoiceMoveStateTrait},
    },
    tts::message::AnnounceMessage,
};
use serenity::{model::voice::VoiceState, prelude::Context};

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

    {
        let mut storage = storage_lock.write().await;
        if !storage.contains_key(&guild_id) {
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
                    del_flag = channel.1.members(&ctx.cache).await.unwrap().len() <= 1;
                }
            }

            if del_flag {
                storage.remove(&guild_id);

                let manager = songbird::get(&ctx)
                    .await
                    .expect("Cannot get songbird client.")
                    .clone();

                manager.remove(guild_id.0).await.unwrap();
            }
        }
    }
}
