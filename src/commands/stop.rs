use crate::{
    errors::{NCBError, Result},
    tts::session::get_session,
};
use serenity::{
    all::{CommandInteraction, EditInteractionResponse, EditThread, ThreadId},
    prelude::Context,
};

pub async fn stop_command(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    let guild_id = command.guild_id.ok_or(NCBError::GuildNotFound)?;
    {
        let guild = ctx.cache.guild(guild_id).ok_or(NCBError::GuildNotFound)?;
        guild
            .voice_states
            .get(&command.user.id)
            .and_then(|state| state.channel_id)
            .ok_or(NCBError::UserNotInVoiceChannel)?;
    }
    let session = get_session(ctx, guild_id).await;
    if let Some(session) = &session {
        session.cancel();
    }
    command.defer(&ctx.http).await?;
    if let Some(session) = session {
        session.stop(ctx).await?;
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("停止しました"),
            )
            .await?;
        if let Some(channel) = session.instance.text_channels.first() {
            let _ = EditThread::new()
                .archived(true)
                .execute(&ctx.http, ThreadId::new(channel.get()))
                .await;
        }
    } else {
        command
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("すでに停止しています"),
            )
            .await?;
    }
    Ok(())
}
