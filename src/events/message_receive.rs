use serenity::{model::prelude::Message, prelude::Context};

use crate::data::TTSData;

pub async fn message(ctx: Context, message: Message) {
    if message.author.bot {
        return;
    }

    let guild_id = message.guild(&ctx.cache);

    if let None = guild_id {
        return;
    }

    let guild_id = guild_id.unwrap().id;

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

        if instance.text_channel != message.channel_id {
            return;
        }

        if message.content.starts_with(";") {
            return;
        }

        instance.read(message, &ctx).await;
    }
}
