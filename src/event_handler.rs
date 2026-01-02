use crate::{errors::NCBError, events, interactions};
use serenity::{
    async_trait,
    client::{Context, EventHandler},
    model::{application::Interaction, channel::Message, gateway::Ready, voice::VoiceState},
};

#[derive(Clone, Debug)]
pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    #[tracing::instrument]
    async fn message(&self, ctx: Context, message: Message) {
        events::message_receive::message(ctx, message).await
    }

    #[tracing::instrument]
    async fn ready(&self, ctx: Context, ready: Ready) {
        events::ready::ready(ctx, ready).await
    }

    #[tracing::instrument]
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Err(e) = interactions::handle_interaction(&ctx, &interaction).await {
            tracing::error!("Error handling interaction: {}", e);

            // Attempt to send error message to user
            if let Err(response_err) = self.send_error_response(&ctx, &interaction, &e).await {
                tracing::error!("Failed to send error response: {}", response_err);
            }
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        events::voice_state_update::voice_state_update(ctx, old, new).await
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
