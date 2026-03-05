use serenity::{model::prelude::Message, model::id::ChannelId, prelude::Context};

use crate::data::UserData;

pub async fn message(ctx: &Context, message: &Message) {
    if message.author.bot() {
        return;
    }

    let guild_id = message.guild(&ctx.cache);

    if let None = guild_id {
        return;
    }

    let guild_id = guild_id.unwrap().id;

    let storage_lock = ctx.data::<UserData>().tts_data.clone();

    {
        let mut storage = storage_lock.write().await;
        if !storage.contains_key(&guild_id) {
            return;
        }

        let instance = storage.get_mut(&guild_id).unwrap();

        if !instance.contains_text_channel(ChannelId::new(message.channel_id.get())) {
            return;
        }

        if message.content.starts_with(";") {
            return;
        }

        instance.read(message.clone(), &ctx).await;
    }
}
