use std::{sync::Arc, time::Duration};

use serenity::all::{
    ChannelId, CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseMessage, EditInteractionResponse,
    GuildId as SerenityGuildId,
};
use tracing::{error, warn};

use crate::transcription::{
    bridge::BridgeHandle,
    receiver::ReceiverRegistry,
    router::{GuildId, UserId, VoiceRouter},
};

const WEB_DRAIN_TTL: Duration = Duration::from_secs(15);

pub struct Transcription {
    pub router: Arc<VoiceRouter>,
    pub bridge: Arc<BridgeHandle>,
    pub web_base_url: Option<String>,
    pub(super) receivers: ReceiverRegistry,
}

impl Transcription {
    pub async fn handle_interaction(&self, ctx: &Context, command: &CommandInteraction) {
        if let Err(error) = command
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(
                    CreateInteractionResponseMessage::new().ephemeral(true),
                ),
            )
            .await
        {
            warn!(%error, "failed to defer transcription command");
            return;
        }
        let content = match self.handle_command(ctx, command).await {
            Ok(message) => message,
            Err(error) => {
                error!(%error, "transcription command failed");
                format!("失敗しました: {error}")
            }
        };
        if let Err(error) = command
            .edit_response(&ctx.http, EditInteractionResponse::new().content(content))
            .await
        {
            warn!(%error, "failed to edit transcription response");
        }
    }

    pub fn voice_channel(&self, guild_id: SerenityGuildId) -> Option<u64> {
        self.router.voice_channel(GuildId(guild_id.get()))
    }

    pub fn stop_router(&self, guild_id: SerenityGuildId, reason: &str) {
        if let Some(grant) = self.router.stop_guild(GuildId(guild_id.get()), reason) {
            let router = Arc::clone(&self.router);
            tokio::spawn(async move {
                tokio::time::sleep(WEB_DRAIN_TTL).await;
                router.finish_web_drain(&grant);
            });
        }
    }

    /// Called under the guild setup lock before every TTS/transcription join.
    pub(crate) async fn install_receiver(
        &self,
        ctx: &Context,
        guild_id: SerenityGuildId,
        voice_channel: ChannelId,
        call: &Arc<tokio::sync::Mutex<songbird::Call>>,
    ) {
        self.receivers
            .install(ctx, guild_id, voice_channel, call, &self.router)
            .await;
    }

    pub async fn voice_state_update(&self, ctx: &Context, state: &serenity::all::VoiceState) {
        let Some(guild) = state.guild_id else {
            return;
        };
        let data = ctx.data::<crate::data::UserData>();
        let _guard = data.setup_guard(guild).await;
        self.receivers
            .voice_state_update(ctx.cache.current_user().id, state);
        let Some(channel) = self.voice_channel(guild) else {
            return;
        };
        if state.user_id == ctx.cache.current_user().id {
            if state.channel_id.map(|id| id.get()) != Some(channel) {
                self.router
                    .abort_guild(GuildId(guild.get()), "bot_left_or_moved");
            }
        } else if state.channel_id.map(|id| id.get()) != Some(channel) {
            self.router
                .disconnect_user(GuildId(guild.get()), UserId(state.user_id.get()));
        }
    }

    pub async fn maintain(&self, ctx: &Context) {
        for (guild, channel) in self.router.active_sessions() {
            let guild_id = SerenityGuildId::new(guild.0);
            let data = ctx.data::<crate::data::UserData>();
            let _guard = data.setup_guard(guild_id).await;
            if self.voice_channel(guild_id) != Some(channel) {
                continue;
            }
            let instance = crate::tts::instance::TTSInstance::new_single(
                ChannelId::new(channel),
                ChannelId::new(channel),
                guild_id,
            );
            match crate::tts::session::voice_presence(ctx, &instance) {
                crate::tts::session::VoicePresence::Unknown => continue,
                crate::tts::session::VoicePresence::Empty => {
                    self.stop_router(guild_id, "empty_channel");
                    if !data.tts_data.read().await.contains_key(&guild_id) {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(10),
                            data.songbird.remove(guild_id),
                        )
                        .await;
                    }
                }
                crate::tts::session::VoicePresence::Occupied => {
                    let call = data.songbird.get_or_insert(guild_id);
                    self.install_receiver(ctx, guild_id, ChannelId::new(channel), &call)
                        .await;
                    if !matches!(
                        tokio::time::timeout(
                            Duration::from_secs(10),
                            data.songbird.join(guild_id, ChannelId::new(channel))
                        )
                        .await,
                        Ok(Ok(_))
                    ) {
                        warn!(guild_id = %guild_id, "transcription voice reconnect deferred");
                    }
                }
            }
        }
    }

    async fn handle_command(
        &self,
        ctx: &Context,
        command: &CommandInteraction,
    ) -> anyhow::Result<String> {
        let Some(option) = command.data.options().into_iter().next() else {
            anyhow::bail!("サブコマンドを指定してください");
        };
        match option.name {
            "start" => self.start(ctx, command).await,
            "stop" => self.stop(ctx, command).await,
            "status" => self.status(command).await,
            #[cfg(feature = "web-ui")]
            "web" if self.web_base_url.is_some() => self.web(ctx, command).await,
            "consent" => self.consent(command).await,
            "revoke" => self.revoke(command).await,
            _ => anyhow::bail!("未知のサブコマンドです"),
        }
    }

    async fn start(&self, ctx: &Context, command: &CommandInteraction) -> anyhow::Result<String> {
        let guild_id = command
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("Guild内で実行してください"))?;
        let data = ctx.data::<crate::data::UserData>();
        let _guard = data.setup_guard(guild_id).await;
        if self.router.is_active(GuildId(guild_id.get())) {
            anyhow::bail!("このGuildでは既に音声認識が動作しています");
        }
        let voice_channel = invoking_voice_channel(ctx, guild_id, command.user.id)
            .ok_or_else(|| anyhow::anyhow!("先に対象ボイスチャンネルへ参加してください"))?;
        let notification_channel = command.channel_id;
        let manager = &data.songbird;
        if let Some(session) = data.tts_data.read().await.get(&guild_id) {
            if session.instance.voice_channel != voice_channel {
                anyhow::bail!("TTSが使用中のボイスチャンネルで開始してください");
            }
        }
        let web_token = self
            .router
            .start_guild(GuildId(guild_id.get()), voice_channel.get());
        let call = manager.get_or_insert(guild_id);
        self.install_receiver(ctx, guild_id, voice_channel, &call)
            .await;
        if let Err(error) = tokio::time::timeout(
            Duration::from_secs(10),
            manager.join(guild_id, voice_channel),
        )
        .await
        .map_err(anyhow::Error::from)
        .and_then(|result| result.map_err(anyhow::Error::from))
        {
            self.router
                .abort_guild(GuildId(guild_id.get()), "join_failed");
            if !data.tts_data.read().await.contains_key(&guild_id) {
                let _ = manager.remove(guild_id).await;
            }
            return Err(error);
        }
        let consent_notice = if self.router.requires_consent() {
            "認識を希望する参加者は `/transcribe consent` を実行してください。撤回は `/transcribe revoke` です。"
        } else {
            "参加者は既定で認識対象です。対象外にする場合は `/transcribe revoke`、再開は `/transcribe consent` です。"
        };
        let web_notice = self
            .web_base_url
            .as_deref()
            .map(|base_url| {
                format!(
                    "\nWeb UI（Discord認証・対象VC参加者のみ）: <{}>",
                    web_url(base_url, &web_token)
                )
            })
            .unwrap_or_default();
        if let Err(error) = notification_channel
            .say(
                &ctx.http,
                format!(
                    "🔴 音声認識を開始しました。参加者ごとの音声をhayamimiで文字起こしします。\n\
             生音声は保存しません。{consent_notice}\n停止は `/transcribe stop` です。{web_notice}"
                ),
            )
            .await
        {
            self.router
                .abort_guild(GuildId(guild_id.get()), "notification_failed");
            if !data.tts_data.read().await.contains_key(&guild_id) {
                let _ =
                    tokio::time::timeout(Duration::from_secs(10), manager.remove(guild_id)).await;
            }
            return Err(error.into());
        }
        Ok(format!(
            "<#{}> に参加しました。hayamimi接続: {}",
            voice_channel.get(),
            if self.bridge.is_connected() {
                "接続済み"
            } else {
                "再接続中"
            }
        ))
    }

    async fn stop(&self, ctx: &Context, command: &CommandInteraction) -> anyhow::Result<String> {
        let guild_id = command
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("Guild内で実行してください"))?;
        let data = ctx.data::<crate::data::UserData>();
        let _guard = data.setup_guard(guild_id).await;
        self.stop_router(guild_id, "stopped");
        if !data.tts_data.read().await.contains_key(&guild_id) {
            tokio::time::timeout(Duration::from_secs(10), data.songbird.remove(guild_id)).await??;
        }
        command
            .channel_id
            .say(&ctx.http, "⏹️ 音声認識を停止しました。")
            .await?;
        Ok("停止しました。".to_owned())
    }

    async fn status(&self, command: &CommandInteraction) -> anyhow::Result<String> {
        let guild_id = command
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("Guild内で実行してください"))?;
        Ok(format!(
            "session: {} / hayamimi: {} / active streams: {} / consent: {}",
            if self.router.is_active(GuildId(guild_id.get())) {
                "active"
            } else {
                "stopped"
            },
            if self.bridge.is_connected() {
                "connected"
            } else {
                "disconnected"
            },
            self.router.active_streams(GuildId(guild_id.get())),
            if self.router.requires_consent() {
                "required"
            } else {
                "enabled-by-default"
            }
        ))
    }

    #[cfg(feature = "web-ui")]
    async fn web(&self, ctx: &Context, command: &CommandInteraction) -> anyhow::Result<String> {
        let guild_id = command
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("Guild内で実行してください"))?;
        let base_url = self
            .web_base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Web UIが設定されていません"))?;
        let token = self
            .router
            .web_token(GuildId(guild_id.get()))
            .ok_or_else(|| anyhow::anyhow!("このGuildでは音声認識が動作していません"))?;
        let grant = self
            .router
            .web_grant(&token)
            .ok_or_else(|| anyhow::anyhow!("Web UIリンクの有効期限が切れています"))?;
        let current_channel = invoking_voice_channel(ctx, guild_id, command.user.id)
            .ok_or_else(|| anyhow::anyhow!("対象ボイスチャンネルへ参加してください"))?;
        if current_channel.get() != grant.voice_channel_id {
            anyhow::bail!("対象ボイスチャンネルへ参加してください");
        }
        Ok(format!(
            "Web UI: <{}>\nリンクを開くにはDiscord認証が必要です。",
            web_url(base_url, &token)
        ))
    }

    async fn consent(&self, command: &CommandInteraction) -> anyhow::Result<String> {
        let guild_id = command
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("Guild内で実行してください"))?;
        if !self
            .router
            .consent(GuildId(guild_id.get()), UserId(command.user.id.get()))
        {
            anyhow::bail!("このGuildでは音声認識が動作していません");
        }
        Ok("音声認識を有効にしました。".to_owned())
    }

    async fn revoke(&self, command: &CommandInteraction) -> anyhow::Result<String> {
        let guild_id = command
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("Guild内で実行してください"))?;
        if !self
            .router
            .revoke(GuildId(guild_id.get()), UserId(command.user.id.get()))
        {
            anyhow::bail!("このGuildでは音声認識が動作していません");
        }
        Ok("同意を撤回しました。以後の音声は送信されません。".to_owned())
    }
}

fn invoking_voice_channel(
    ctx: &Context,
    guild_id: SerenityGuildId,
    user_id: serenity::all::UserId,
) -> Option<ChannelId> {
    let guild = ctx.cache.guild(guild_id)?;
    guild.voice_states.get(&user_id)?.channel_id
}

pub fn transcribe_command(web_enabled: bool) -> CreateCommand<'static> {
    let command = CreateCommand::new("transcribe")
        .description("参加者別のリアルタイム文字起こしを管理します")
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "start",
            "現在のボイスチャンネルで開始します",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "stop",
            "文字起こしを停止します（TTSは継続）",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "status",
            "接続状態を表示します",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "consent",
            "自分の音声認識に同意します",
        ))
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "revoke",
            "自分の同意を撤回します",
        ));
    if web_enabled {
        command.add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "web",
            "対象ボイスチャンネル参加者用のWeb UIリンクを表示します",
        ))
    } else {
        command
    }
}

fn web_url(base_url: &str, token: &str) -> String {
    format!("{}/view/{token}", base_url.trim_end_matches('/'))
}
