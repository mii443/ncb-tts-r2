use crate::{
    data::UserData,
    database::{database::Database, server_config::ServerConfig, user_config::UserConfig},
    errors::{NCBError, Result},
};
use serenity::{
    all::{
        ButtonStyle, ComponentInteraction, CreateActionRow, CreateButton, CreateComponent,
        CreateInteractionResponse, CreateInteractionResponseMessage, LabelComponent,
        ModalComponent,
    },
    prelude::Context,
};
use std::sync::Arc;

pub async fn get_database_client(ctx: &Context) -> Result<Arc<Database>> {
    Ok(ctx.data::<UserData>().database.clone())
}

pub async fn get_server_config(ctx: &Context, guild_id: u64) -> Result<ServerConfig> {
    let database = get_database_client(ctx).await?;
    database
        .get_server_config_or_default(guild_id)
        .await?
        .ok_or_else(|| NCBError::config("Server config not found"))
}

pub async fn set_server_config(
    ctx: &Context,
    guild_id: u64,
    config: ServerConfig,
) -> Result<()> {
    let database = get_database_client(ctx).await?;
    database.set_server_config(guild_id, config).await?;
    Ok(())
}

pub async fn get_user_config(ctx: &Context, user_id: u64) -> Result<UserConfig> {
    let database = get_database_client(ctx).await?;
    database
        .get_user_config_or_default(user_id)
        .await?
        .ok_or_else(|| NCBError::config("User config not found"))
}

pub async fn set_user_config(ctx: &Context, user_id: u64, config: UserConfig) -> Result<()> {
    let database = get_database_client(ctx).await?;
    database.set_user_config(user_id, config).await?;
    Ok(())
}

pub async fn update_interaction_message(
    ctx: &Context,
    interaction: &ComponentInteraction,
    content: impl Into<String>,
) -> Result<()> {
    let content_string: String = content.into();
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new().content(content_string),
            ),
        )
        .await
        .map_err(|e| NCBError::Discord(e))?;
    Ok(())
}

pub fn build_server_config_buttons() -> Vec<CreateComponent<'static>> {
    use crate::errors::constants::*;

    vec![CreateComponent::ActionRow(CreateActionRow::Buttons(vec![
        CreateButton::new(TTS_CONFIG_SERVER_DICTIONARY)
            .label("辞書管理")
            .style(ButtonStyle::Primary),
        CreateButton::new(TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL)
            .label("自動参加チャンネル")
            .style(ButtonStyle::Primary),
        CreateButton::new(TTS_CONFIG_SERVER_SET_VOICE_STATE_ANNOUNCE)
            .label("入退出アナウンス通知切り替え")
            .style(ButtonStyle::Primary),
        CreateButton::new(TTS_CONFIG_SERVER_SET_READ_USERNAME)
            .label("ユーザー名読み上げ切り替え")
            .style(ButtonStyle::Primary),
    ].into()))]
}

pub fn build_dictionary_menu_buttons() -> Vec<CreateComponent<'static>> {
    use crate::errors::constants::*;

    vec![
        CreateComponent::ActionRow(CreateActionRow::Buttons(vec![
            CreateButton::new(TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON)
                .label("辞書を追加")
                .style(ButtonStyle::Primary),
            CreateButton::new(TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON)
                .label("辞書を削除")
                .style(ButtonStyle::Danger),
            CreateButton::new(TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON)
                .label("辞書一覧")
                .style(ButtonStyle::Primary),
        ].into())),
        CreateComponent::ActionRow(CreateActionRow::Buttons(vec![CreateButton::new(TTS_CONFIG_SERVER_BACK)
            .label("← サーバー設定に戻る")
            .style(ButtonStyle::Secondary)].into())),
    ]
}

pub fn build_back_button() -> CreateComponent<'static> {
    use crate::errors::constants::TTS_CONFIG_SERVER_BACK;

    CreateComponent::ActionRow(CreateActionRow::Buttons(vec![CreateButton::new(TTS_CONFIG_SERVER_BACK)
        .label("← サーバー設定に戻る")
        .style(ButtonStyle::Secondary)].into()))
}

pub fn extract_guild_id(interaction: &ComponentInteraction) -> Result<u64> {
    interaction
        .guild_id
        .ok_or_else(|| NCBError::config("Guild not found"))
        .map(|id| id.get())
}

pub fn parse_select_value(interaction: &ComponentInteraction, prefix: &str) -> Result<u64> {
    use serenity::all::ComponentInteractionDataKind;

    if let ComponentInteractionDataKind::StringSelect { ref values, .. } = interaction.data.kind {
        if values.is_empty() {
            return Ok(0);
        }

        if values[0].ends_with("_CLEAR") {
            return Ok(0);
        }

        let value = values[0]
            .strip_prefix(prefix)
            .ok_or_else(|| NCBError::invalid_input("Invalid select value prefix"))?;

        value
            .parse::<u64>()
            .map_err(|_| NCBError::invalid_input("Failed to parse channel ID"))
    } else {
        Err(NCBError::invalid_input(
            "Not a string select interaction",
        ))
    }
}

pub fn extract_input_text(
    components: &[ModalComponent],
    row_index: usize,
    _component_index: usize,
) -> Option<String> {
    let component = components.get(row_index)?;
    match component {
        ModalComponent::Label(label) => {
            if let LabelComponent::InputText(text) = &label.component {
                text.value.as_ref().map(|v| v.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn parse_select_index(interaction: &ComponentInteraction) -> Result<usize> {
    use serenity::all::ComponentInteractionDataKind;

    if let ComponentInteractionDataKind::StringSelect { ref values, .. } = interaction.data.kind {
        if values.is_empty() {
            return Err(NCBError::invalid_input("No value selected"));
        }

        values[0]
            .parse::<usize>()
            .map_err(|_| NCBError::invalid_input("Failed to parse index"))
    } else {
        Err(NCBError::invalid_input(
            "Not a string select interaction",
        ))
    }
}
