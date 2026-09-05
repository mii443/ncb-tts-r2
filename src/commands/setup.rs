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
        AutoArchiveDuration, ChannelId, CommandDataOptionValue, CommandInteraction, CreateEmbed,
        CreateMessage, CreateThread, EditInteractionResponse,
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
    command
        .edit_response(
            &ctx.http,
            EditInteractionResponse::new().content(format!(
                "TTS Channel: <#{}>{}",
                text_channel,
                if text_channel == channel_id {
                    "\nボイスチャンネルのチャットを開いて利用できます。"
                } else {
                    ""
                },
            )),
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
    text_channel
        .widen()
        .send_message(
            &ctx.http,
            CreateMessage::new().embed(
                CreateEmbed::new()
                    .title("読み上げ (Serenity)")
                    .field(
                        "VOICEVOXクレジット",
                        format!("```\n{}\n```", speakers.join("\n")),
                        false,
                    )
                    .field("設定コマンド", "`/config`", false)
                    .field("フィードバック", "https://feedback.mii.codes/", false),
            ),
        )
        .await?;
    Ok(())
}
