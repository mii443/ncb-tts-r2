use serenity::{
    all::{
        ChannelId, CreateAllowedMentions, CreateEmbed, CreateMessage, MessageFlags, Permissions,
    },
    http::{Http, HttpError, JsonErrorCode},
};

/// Credits are a notification after connecting, not a requirement for TTS.
/// Interaction permissions apply only when the notice targets that same channel.
pub(crate) async fn send_credits(
    http: &Http,
    channel: ChannelId,
    title: &str,
    speakers: &[String],
    permissions: Option<Permissions>,
) -> serenity::Result<()> {
    let credits = speakers.join("\n");
    let field = format!("```\n{credits}\n```");
    let can_embed = permissions
        .is_none_or(|p| p.intersects(Permissions::ADMINISTRATOR | Permissions::EMBED_LINKS));
    if can_embed && field.chars().count() <= 1024 {
        let message = CreateMessage::new().embed(
            CreateEmbed::new()
                .title(title)
                .field("VOICEVOXクレジット", field, false)
                .field("設定コマンド", "`/config`", false)
                .field("フィードバック", "https://feedback.mii.codes/", false),
        );
        match channel.widen().send_message(http, message).await {
            Ok(_) => return Ok(()),
            // Permissions may have changed since the interaction, or be unknown
            // for autostart/another channel. Retry once without embeds on 50013.
            Err(error) if is_missing_permissions(&error) => {}
            Err(error) => return Err(error),
        }
    }
    let content = format!(
        "{title}\nVOICEVOXクレジット\n{credits}\n設定コマンド: /config\nフィードバック: https://feedback.mii.codes/"
    );
    // Preserve all credits, including when the list exceeds Discord's content limit.
    let mut chars = content.chars().peekable();
    while chars.peek().is_some() {
        let chunk: String = chars.by_ref().take(2000).collect();
        channel
            .widen()
            .send_message(
                http,
                CreateMessage::new()
                    .content(chunk)
                    .flags(MessageFlags::SUPPRESS_EMBEDS)
                    .allowed_mentions(CreateAllowedMentions::new()),
            )
            .await?;
    }
    Ok(())
}

fn is_missing_permissions(error: &serenity::Error) -> bool {
    matches!(error,
        serenity::Error::Http(HttpError::UnsuccessfulRequest(response))
            if response.error.code == JsonErrorCode::LackPermissionsForAction
    )
}

#[cfg(test)]
mod tests;
