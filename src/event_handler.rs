use crate::{errors::NCBError, events, interactions};
use serenity::{
    async_trait,
    model::{application::Interaction, channel::Message, gateway::Ready, voice::VoiceState},
    prelude::{Context, EventHandler},
};

use serenity::gateway::client::FullEvent;

#[derive(Clone, Debug)]
pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn dispatch(&self, ctx: &Context, event: &FullEvent) {
        match event {
            FullEvent::Message { new_message } => {
                events::message_receive::message(ctx, new_message).await;
            }
            FullEvent::Ready { data_about_bot } => {
                events::ready::ready(ctx, data_about_bot).await;
            }
            FullEvent::InteractionCreate { interaction } => {
                if let Err(e) = interactions::handle_interaction(ctx, interaction).await {
                    tracing::error!("Error handling interaction: {}", e);
                    if let Err(response_err) = self.send_error_response(ctx, interaction, &e).await
                    {
                        tracing::error!("Failed to send error response: {}", response_err);
                    }
                }
            }
            FullEvent::VoiceStateUpdate { old, new } => {
                events::voice_state_update::voice_state_update(ctx, old.clone(), new.clone()).await;
            }
            _ => {}
        }
    }
}

impl Handler {
    async fn send_error_response(
        &self,
        ctx: &Context,
        interaction: &Interaction,
        error: &NCBError,
    ) -> crate::errors::Result<()> {
        use serenity::all::{CreateInteractionResponse, CreateInteractionResponseMessage};

        let error_message = match error {
            NCBError::InvalidInput(msg) => format!("入力エラー: {}", msg),
            NCBError::Database(_) => "データベースエラーが発生しました".to_string(),
            NCBError::Config(msg) => format!("設定エラー: {}", msg),
            _ => "予期しないエラーが発生しました".to_string(),
        };

        match interaction {
            Interaction::Command(cmd) => {
                cmd.create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(error_message)
                            .ephemeral(true),
                    ),
                )
                .await
                .map_err(|e| NCBError::Discord(e))?;
            }
            Interaction::Component(comp) => {
                comp.create_response(
                    &ctx.http,
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new().content(error_message),
                    ),
                )
                .await
                .map_err(|e| NCBError::Discord(e))?;
            }
            Interaction::Modal(modal) => {
                modal
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new().content(error_message),
                        ),
                    )
                    .await
                    .map_err(|e| NCBError::Discord(e))?;
            }
            _ => {}
        }

        Ok(())
    }
}
