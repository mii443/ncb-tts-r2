use crate::{
    errors::{NCBError, Result},
    tts::session::get_session,
};
use serenity::{
    all::{CommandInteraction, CreateInteractionResponse, CreateInteractionResponseMessage},
    prelude::Context,
};

pub async fn skip_command(ctx: &Context, command: &CommandInteraction) -> Result<()> {
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
    let response = if let Some(session) = session {
        session.skip();
        "スキップしました"
    } else {
        "読み上げしていません"
    };
    command
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(response),
            ),
        )
        .await?;
    Ok(())
}
