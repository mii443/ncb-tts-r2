use serenity::{client::{EventHandler, Context}, async_trait, model::{gateway::Ready, interactions::{Interaction, application_command::ApplicationCommandInteraction, InteractionApplicationCommandCallbackDataFlags}, id::{GuildId, UserId}, channel::Message, prelude::Member, voice::VoiceState}, framework::standard::macros::group};
use crate::{data::TTSData, tts::{instance::TTSInstance, message::AnnounceMessage}, implement::member_name::ReadName};

#[group]
struct Test;

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

    let channel_id = channel_id.unwrap();

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

            let mut message: Option<String> = None;

            match old {
                Some(old) => {
                    match (old.channel_id, new.channel_id) {
                        (Some(old_channel_id), Some(new_channel_id)) => {
                            if old_channel_id == new_channel_id {
                                return;
                            }
                            if old_channel_id != new_channel_id {
                                if instance.voice_channel == new_channel_id {
                                    message = Some(format!("{} さんが通話に参加しました", new.member.unwrap().read_name()));
                                }
                            } else if old_channel_id == instance.voice_channel && new_channel_id != instance.voice_channel {
                                message = Some(format!("{} さんが通話から退出しました", new.member.unwrap().read_name()));
                            } else {
                                return;
                            }
                        }
                        (Some(old_channel_id), None) => {
                            if old_channel_id == instance.voice_channel {
                                message = Some(format!("{} さんが通話から退出しました", new.member.unwrap().read_name()));
                            } else {
                                return;
                            }
                        }
                        (None, Some(new_channel_id)) => {
                            if new_channel_id == instance.voice_channel {
                                message = Some(format!("{} さんが通話に参加しました", new.member.unwrap().read_name()));
                            } else {
                                return;
                            }
                        }
                        _ => {
                            return;
                        }
                    }
                }
                None => {
                    match new.channel_id {
                        Some(channel_id) => {
                            if instance.voice_channel == channel_id {
                                message = Some(format!("{} さんが通話に参加しました", new.member.unwrap().read_name()));
                            }
                        }
                        None => {
                            return;
                        }
                    }
                }
            }

            if let Some(message) = message {
                instance.read(AnnounceMessage {
                    message
                }, &ctx).await;
            }
        }
    }

    async fn message(&self, ctx: Context, message: Message) {

        if message.author.bot {
            return;
        }

        let guild_id = message.guild(&ctx.cache).await;

        if let None = guild_id {
            return;
        }

        let guild_id = guild_id.unwrap().id;

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

            if instance.text_channel.0 != message.channel_id.0 {
                return;
            }

            instance.read(message, &ctx).await;
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        let guild_id = GuildId(660046656934248460);

        let commands = GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
            commands.create_application_command(|command| {
                command.name("stop")
                    .description("Stop tts")
            });
            commands.create_application_command(|command| {
                command.name("setup")
                    .description("Setup tts")
            })
        }).await;
        println!("{:?}", commands);
    }
}
