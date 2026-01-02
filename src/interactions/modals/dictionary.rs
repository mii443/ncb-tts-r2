//! Dictionary modal handler
//!
//! Handles the "Add Dictionary" modal submission (TTS_CONFIG_SERVER_ADD_DICTIONARY)

use crate::{
    database::dictionary::Rule,
    errors::{validation, NCBError, Result},
    interactions::utils,
};
use serenity::{all::ModalInteraction, prelude::Context};

/// Handle dictionary addition modal submission
/// Extracts and validates rule name, regex pattern, and replacement text
pub async fn handle_add_dictionary(ctx: &Context, modal: &ModalInteraction) -> Result<()> {
    let rows = &modal.data.components;

    // Extract rule name with validation
    let rule_name = utils::extract_input_text(rows, 0, 0)
        .ok_or_else(|| NCBError::invalid_input("Cannot extract rule name from modal"))?;
    validation::validate_rule_name(&rule_name)?;

    // Extract 'from' field with validation
    let from = utils::extract_input_text(rows, 1, 0)
        .ok_or_else(|| NCBError::invalid_input("Cannot extract regex pattern from modal"))?;
    validation::validate_regex_pattern(&from)?;

    // Extract 'to' field with validation
    let to = utils::extract_input_text(rows, 2, 0)
        .ok_or_else(|| NCBError::invalid_input("Cannot extract replacement text from modal"))?;
    validation::validate_replacement_text(&to)?;

    // Create rule
    let rule = Rule {
        id: rule_name.clone(),
        is_regex: true,
        rule: from.clone(),
        to: to.clone(),
    };

    // Get guild ID
    let guild_id = modal
        .guild_id
        .ok_or_else(|| NCBError::config("Guild not found"))?
        .get();

    // Update server config
    let mut config = utils::get_server_config(ctx, guild_id).await?;
    config.dictionary.rules.push(rule);
    utils::set_server_config(ctx, guild_id, config).await?;

    // Send success response
    modal
        .create_response(
            &ctx.http,
            serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new().content(format!(
                    "辞書を追加しました\n名前: {}\n変換元: {}\n変換後: {}",
                    rule_name, from, to
                )),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;

    Ok(())
}
