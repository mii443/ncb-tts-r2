//! Modal interaction handlers

mod dictionary;

use crate::errors::Result;
use serenity::{all::ModalInteraction, prelude::Context};

/// Handle modal submissions
pub async fn handle_modal(ctx: &Context, modal: &ModalInteraction) -> Result<()> {
    match modal.data.custom_id.as_str() {
        crate::errors::constants::TTS_CONFIG_SERVER_ADD_DICTIONARY => {
            dictionary::handle_add_dictionary(ctx, modal).await
        }
        _ => Ok(()),
    }
}
