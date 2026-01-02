//! Dictionary management button handlers
//!
//! Handles buttons for adding, removing, and showing dictionary entries

use crate::{errors::{NCBError, Result}, interactions::utils};
use serenity::{
    all::{
        ComponentInteraction, CreateActionRow, CreateEmbed, CreateInputText,
        CreateInteractionResponse, CreateInteractionResponseMessage, CreateModal,
        CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, InputTextStyle,
    },
    prelude::Context,
};

/// Handle "TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON" - show modal for adding dictionary
pub async fn handle_show_add_modal(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Modal(
                CreateModal::new("TTS_CONFIG_SERVER_ADD_DICTIONARY", "辞書追加").components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "rule_name", "辞書名")
                            .required(true),
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(
                            InputTextStyle::Paragraph,
                            "from",
                            "変換元（正規表現）",
                        )
                        .required(true),
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(InputTextStyle::Short, "to", "変換先")
                            .required(true),
                    ),
                ]),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;
    Ok(())
}

/// Handle "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON" - show select menu for removing
pub async fn handle_show_remove_menu(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let guild_id = utils::extract_guild_id(interaction)?;
    let config = utils::get_server_config(ctx, guild_id).await?;

    let mut options = vec![];
    for (i, rule) in config.dictionary.rules.iter().enumerate() {
        let option = CreateSelectMenuOption::new(rule.id.clone(), i.to_string())
            .description(format!("{} -> {}", rule.rule, rule.to));
        options.push(option);
    }

    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("削除する辞書内容を選択してください")
                    .components(vec![CreateActionRow::SelectMenu(
                        CreateSelectMenu::new(
                            "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU",
                            CreateSelectMenuKind::String { options },
                        )
                        .max_values(1)
                        .min_values(0),
                    )]),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;
    Ok(())
}

/// Handle "TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON" - show dictionary list
pub async fn handle_show_dictionary_list(
    ctx: &Context,
    interaction: &ComponentInteraction,
) -> Result<()> {
    let guild_id = utils::extract_guild_id(interaction)?;
    let config = utils::get_server_config(ctx, guild_id).await?;

    let mut fields = vec![];
    for rule in config.dictionary.rules {
        let field = (rule.id.clone(), format!("{} -> {}", rule.rule, rule.to), true);
        fields.push(field);
    }

    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("")
                    .embed(CreateEmbed::new().title("辞書一覧").fields(fields)),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;
    Ok(())
}
