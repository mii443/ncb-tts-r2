use serenity::{client::{EventHandler, Context}, async_trait, model::{gateway::Ready, interactions::Interaction, id::GuildId, channel::Message, voice::VoiceState}};
use crate::{events, commands::{setup::setup_command, stop::stop_command}};

pub struct Handler;

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
        events::voice_state_update::voice_state_update(ctx, guild_id, old, new).await
    }
}
