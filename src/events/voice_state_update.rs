use crate::{
    data::UserData,
    implement::{
        member_name::ReadName,
        voice_move_state::{VoiceMoveState, VoiceMoveStateTrait},
    },
    tts::{
        instance::TTSInstance,
        message::AnnounceMessage,
        session::{get_session, voice_presence, TTSSession, VoicePresence},
    },
};
use serenity::{
    all::{CreateEmbed, CreateMessage, EditThread, ThreadId},
    model::voice::VoiceState,
    prelude::Context,
};

pub async fn voice_state_update(ctx: &Context, old: Option<VoiceState>, new: VoiceState) {
    if new.member.as_ref().is_some_and(|member| member.user.bot()) {
        return;
    }
    let Some(guild_id) = new
        .guild_id
        .or_else(|| old.as_ref().and_then(|old| old.guild_id))
    else {
        return;
    };
    let data = ctx.data::<UserData>();
    let config = match data
        .database
        .get_server_config_or_default(guild_id.get())
        .await
    {
        Ok(Some(config)) => config,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(guild_id = %guild_id, error = %error, "Cannot read voice settings");
            return;
        }
    };
    let session = if let Some(session) = get_session(ctx, guild_id).await {
        session
    } else {
        let Some(channel) = new.channel_id else {
            return;
        };
        if config.autostart_channel_id != Some(channel.get()) {
            return;
        }
        let setup_guard = data.setup_guard(guild_id).await;
        if get_session(ctx, guild_id).await.is_some() {
            return;
        }
        let mut channels = vec![channel];
        if let Some(text) = config.autostart_text_channel_id {
            let text = text.into();
            if text != channel {
                channels.insert(0, text);
            }
        }
        let session = TTSSession::new(
            TTSInstance::new(channels, channel, guild_id),
            ctx.clone(),
            false,
        );
        data.tts_data
            .write()
            .await
            .insert(guild_id, session.clone());
        drop(setup_guard);
        if let Err(error) = session.connect(ctx).await {
            tracing::warn!(guild_id = %guild_id, error = %error, "Autostart connection will be retried");
            return;
        }
        let speakers = data
            .tts_client
            .voicevox_client
            .get_speakers()
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(error = %error, "Cannot fetch VOICEVOX credits");
                vec!["VOICEVOX API unavailable".into()]
            });
        let message = CreateMessage::new().embed(
            CreateEmbed::new()
                .title("自動参加 読み上げ（Serenity）")
                .field(
                    "VOICEVOXクレジット",
                    format!("```\n{}\n```", speakers.join("\n")),
                    false,
                )
                .field("設定コマンド", "`/config`", false)
                .field("フィードバック", "https://feedback.mii.codes/", false),
        );
        if let Err(error) = channel.widen().send_message(&ctx.http, message).await {
            tracing::warn!(error = %error, "Cannot send autostart notification");
        }
        return;
    };
    let movement = new.move_state(&old, session.instance.voice_channel);
    if config.voice_state_announce.unwrap_or(false) {
        if let Some(member) = new.member.as_ref() {
            let message = match movement {
                VoiceMoveState::JOIN => {
                    Some(format!("{} さんが通話に参加しました", member.read_name()))
                }
                VoiceMoveState::LEAVE => {
                    Some(format!("{} さんが通話から退出しました", member.read_name()))
                }
                _ => None,
            };
            if let Some(message) = message {
                if let Err(error) = session.enqueue(AnnounceMessage { message }) {
                    tracing::warn!(error = %error, "Cannot queue voice announcement");
                }
            }
        }
    }
    if movement == VoiceMoveState::LEAVE
        && voice_presence(ctx, &session.instance) == VoicePresence::Empty
    {
        if let Err(error) = session.stop(ctx).await {
            tracing::warn!(error = %error, "Session cleanup will be retried");
        } else if let Some(channel) = session.instance.text_channels.first() {
            let _ = EditThread::new()
                .archived(true)
                .execute(&ctx.http, ThreadId::new(channel.get()))
                .await;
        }
    }
}
