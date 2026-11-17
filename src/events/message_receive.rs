use crate::{data::UserData, tts::session::get_session};
use serenity::{
    model::{id::ChannelId, prelude::Message},
    prelude::Context,
};

pub async fn message(ctx: &Context, message: &Message) {
    let data = ctx.data::<UserData>();
    if (message.author.bot() && !data.readable_bot_ids.contains(&message.author.id.get()))
        || message.content.starts_with(';')
    {
        return;
    }
    let Some(guild) = message.guild_id else {
        return;
    };
    let Some(session) = get_session(ctx, guild).await else {
        return;
    };
    if !session
        .instance
        .contains_text_channel(ChannelId::new(message.channel_id.get()))
    {
        return;
    }
    if let Err(error) = session.enqueue(message.clone()) {
        tracing::warn!(guild_id = %guild, error = %error, "Unable to queue message");
    }
}
