use crate::{
    data::UserData,
    errors::{NCBError, Result},
    tts::{
        instance::TTSInstance,
        session::{get_session, TTSSession},
    },
};
use serenity::{
    all::{
        AutoArchiveDuration, ChannelId, CommandDataOptionValue, CommandInteraction, CreateThread,
        EditInteractionResponse,
    },
    prelude::Context,
};

#[tracing::instrument(skip_all)]
pub async fn setup_command(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let guild_id = command.guild_id.ok_or(NCBError::GuildNotFound)?;
    let channel_id = {
        let guild = ctx.cache.guild(guild_id).ok_or(NCBError::GuildNotFound)?;
        guild
            .voice_states
            .get(&command.user.id)
            .and_then(|state| state.channel_id)
            .ok_or(NCBError::UserNotInVoiceChannel)?
    };
    command.defer(&ctx.http).await?;
    let data = ctx.data::<UserData>();
    let setup_guard = data.setup_guard(guild_id).await;
    #[cfg(feature = "transcription")]
    crate::transcription::ensure_channel(&data, guild_id, channel_id)?;
    if get_session(ctx, guild_id).await.is_some() {
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("すでにセットアップしています。"),
            )
            .await?;
        return Ok(());
    }
    let command_channel = ChannelId::new(command.channel_id.get());
    let mode = command
        .data
        .options
        .first()
        .and_then(|option| match &option.value {
            CommandDataOptionValue::String(mode) => Some(mode.as_str()),
            _ => None,
        });
    let text_channels = match mode {
        Some("TEXT_CHANNEL") => vec![command_channel],
        Some("VOICE_CHANNEL") => vec![channel_id],
        Some("NEW_THREAD") => {
            let thread = command_channel
                .create_thread(
                    &ctx.http,
                    CreateThread::new("TTS")
                        .auto_archive_duration(AutoArchiveDuration::OneHour)
                        .kind(serenity::all::ChannelType::PublicThread),
                )
                .await?;
            vec![ChannelId::new(thread.id.get())]
        }
        _ if command_channel != channel_id => vec![command_channel, channel_id],
        _ => vec![channel_id],
    };
    let text_channel = text_channels[0];
    let session = TTSSession::new(
        TTSInstance::new(text_channels, channel_id, guild_id),
        ctx.clone(),
        false,
    );
    data.tts_data
        .write()
        .await
        .insert(guild_id, session.clone());
    drop(setup_guard);
    session.connect(ctx).await?;
    let mut response = format!(
        "TTS Channel: <#{}>{}",
        text_channel,
        if text_channel == channel_id {
            "\nボイスチャンネルのチャットを開いて利用できます。"
        } else {
            ""
        },
    );
    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content(response.clone()),
        )
        .await?;
    let speakers = data
        .tts_client
        .voicevox_client
        .get_speakers()
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(error = %error, "Cannot fetch VOICEVOX credits");
            vec!["VOICEVOX API unavailable".into()]
        });
    let notice = crate::tts::notice::send_credits(
        &ctx.http,
        text_channel,
        "読み上げ (Serenity)",
        &speakers,
        (text_channel == command_channel).then_some(command.app_permissions),
    )
    .await;
    if let Err(error) = notice {
        tracing::warn!(%guild_id, channel_id = %text_channel, %error,
            "TTS connected but setup notification failed");
        response.push_str("\n読み上げは開始しましたが、案内メッセージを投稿できませんでした。Botの投稿権限と接続状況を確認してください。");
        if let Err(error) = command
            .edit_response(&ctx.http, EditInteractionResponse::new().content(response))
            .await
        {
            tracing::warn!(%guild_id, %error, "Cannot update setup notification warning");
        }
    }
    Ok(())
}
