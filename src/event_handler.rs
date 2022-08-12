use serenity::{client::{EventHandler, Context}, async_trait, model::{gateway::Ready, interactions::{Interaction, application_command::ApplicationCommandInteraction, InteractionApplicationCommandCallbackDataFlags}, id::{GuildId, UserId}, channel::Message, voice::VoiceState}};
use crate::{data::TTSData, tts::{instance::TTSInstance, message::AnnounceMessage}, implement::{member_name::ReadName, voice_move_state::{VoiceMoveStateTrait, VoiceMoveState}}, events};

pub struct Handler;

async fn stop_command(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<(), Box<dyn std::error::Error>> {
    if let None = command.guild_id {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("このコマンドはサーバーでのみ使用可能です．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }

    let guild = command.guild_id.unwrap().to_guild_cached(&ctx.cache).await;
    if let None = guild {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("ギルドキャッシュを取得できませんでした．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }
    let guild = guild.unwrap();

    let channel_id = guild
        .voice_states
        .get(&UserId(command.user.id.0))
        .and_then(|state| state.channel_id);

    if let None = channel_id {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("ボイスチャンネルに参加してから実行してください．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }

    let manager = songbird::get(ctx).await.expect("Cannot get songbird client.").clone();

    let storage_lock = {
        let data_read = ctx.data.read().await;
        data_read.get::<TTSData>().expect("Cannot get TTSStorage").clone()
    };

    {
        let mut storage = storage_lock.write().await;
        if !storage.contains_key(&guild.id) {
            command.create_interaction_response(&ctx.http, |f| {
                f.interaction_response_data(|d| {
                    d.content("すでに停止しています").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                })
            }).await?;
            return Ok(());
        }

        storage.remove(&guild.id);
    }

    let _handler = manager.leave(guild.id.0).await;

    command.create_interaction_response(&ctx.http, |f| {
        f.interaction_response_data(|d| {
            d.content("停止しました").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
        })
    }).await?;

    Ok(())
}

async fn setup_command(ctx: &Context, command: &ApplicationCommandInteraction) -> Result<(), Box<dyn std::error::Error>> {
    if let None = command.guild_id {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("このコマンドはサーバーでのみ使用可能です．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }

    let guild = command.guild_id.unwrap().to_guild_cached(&ctx.cache).await;
    if let None = guild {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("ギルドキャッシュを取得できませんでした．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }
    let guild = guild.unwrap();

    let channel_id = guild
        .voice_states
        .get(&UserId(command.user.id.0))
        .and_then(|state| state.channel_id);

    if let None = channel_id {
        command.create_interaction_response(&ctx.http, |f| {
            f.interaction_response_data(|d| {
                d.content("ボイスチャンネルに参加してから実行してください．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
        }).await?;
        return Ok(());
    }

    let channel_id = channel_id.unwrap();

    let manager = songbird::get(ctx).await.expect("Cannot get songbird client.").clone();

    let storage_lock = {
        let data_read = ctx.data.read().await;
        data_read.get::<TTSData>().expect("Cannot get TTSStorage").clone()
    };

    {
        let mut storage = storage_lock.write().await;
        if storage.contains_key(&guild.id) {
            command.create_interaction_response(&ctx.http, |f| {
                f.interaction_response_data(|d| {
                    d.content("すでにセットアップしています．").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                })
            }).await?;
            return Ok(());
        }

        storage.insert(guild.id, TTSInstance {
            before_message: None,
            guild: guild.id,
            text_channel: command.channel_id,
            voice_channel: channel_id
        });
    }

    let _handler = manager.join(guild.id.0, channel_id.0).await;

    command.create_interaction_response(&ctx.http, |f| {
        f.interaction_response_data(|d| {
            d.content("セットアップ完了").flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
        })
    }).await?;

    Ok(())
}

#[async_trait]
impl EventHandler for Handler {

    async fn message(&self, ctx: Context, message: Message) {
        events::message_receive::message(ctx, message).await
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        events::ready::ready(ctx, ready).await
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::ApplicationCommand(command) = interaction {
            let name = &*command.data.name;
            match name {
                "setup" => setup_command(&ctx, &command).await.unwrap(),
                "stop" => stop_command(&ctx, &command).await.unwrap(),
                _ => {}
            }
        }
    }

    async fn voice_state_update(
        &self,
        ctx: Context,
        guild_id: Option<GuildId>,
        old: Option<VoiceState>,
        new: VoiceState,
    ) {
        let guild_id = guild_id.unwrap();

        let storage_lock = {
            let data_read = ctx.data.read().await;
            data_read.get::<TTSData>().expect("Cannot get TTSStorage").clone()
        };

        {
            let mut storage = storage_lock.write().await;
            if !storage.contains_key(&guild_id) {
                return;
            }

            let instance = storage.get_mut(&guild_id).unwrap();

            let voice_move_state = new.move_state(&old, instance.voice_channel);

            let message: Option<String> = match voice_move_state {
                VoiceMoveState::JOIN => Some(format!("{} さんが通話に参加しました", new.member.unwrap().read_name())),
                VoiceMoveState::LEAVE => Some(format!("{} さんが通話から退出しました", new.member.unwrap().read_name())),
                _ => None,
            };

            if let Some(message) = message {
                instance.read(AnnounceMessage {
                    message
                }, &ctx).await;
            }
        }
    }
}
