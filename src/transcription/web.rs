use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Context as _;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{sse::Event, Html, IntoResponse, Redirect, Response, Sse},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures::stream;
use hmac::{Hmac, Mac};
use reqwest::Client;
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;
use serenity::{
    all::{GuildId as SerenityGuildId, UserId as SerenityUserId},
    cache::Cache,
};
use sha2::Sha256;
use tokio::sync::{broadcast, Semaphore};
use tracing::{error, info, warn};

use crate::transcription::{
    protocol::ServerMessage,
    router::{VoiceRouter, WebGrant},
};

const OAUTH_STATE_TTL: Duration = Duration::from_secs(10 * 60);
const WEB_IDENTITY_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_OAUTH_STATES: usize = 1_024;
const MAX_SSE_CONNECTIONS: usize = 256;
const MAX_EVENT_HISTORY: usize = 4_096;
const MAX_EVENT_HISTORY_BYTES: usize = 8 * 1024 * 1024;
const MAX_WEB_EVENT_BYTES: usize = 256 * 1024;
const EVENT_HISTORY_TTL: Duration = Duration::from_secs(5 * 60);
const OAUTH_STATE_COOKIE: &str = "rstt_oauth_state";
const SESSION_COOKIE: &str = "rstt_session";
const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
const DISCORD_TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
const DISCORD_REVOKE_URL: &str = "https://discord.com/api/oauth2/token/revoke";
const DISCORD_CURRENT_USER_URL: &str = "https://discord.com/api/v10/users/@me";
const SESSION_SIGNATURE_DOMAIN: &[u8] = b"rstt-web-identity-v1\0";
const MAX_SESSION_TOKEN_LEN: usize = 128;

const DASHBOARD_HTML: &str = include_str!("web/dashboard.html");
const DASHBOARD_JS: &str = include_str!("web/dashboard.js");
const DASHBOARD_CSS: &str = include_str!("web/dashboard.css");

pub struct WebConfig {
    bind: SocketAddr,
    base_url: Url,
    client_id: String,
    client_secret: String,
}

impl WebConfig {
    pub fn new(config: &crate::config::WebConfig) -> anyhow::Result<Self> {
        let base_url =
            Url::parse(config.base_url.trim()).context("NCB_WEB_BASE_URL is invalid or missing")?;
        if base_url.scheme() != "https"
            && !(base_url.scheme() == "http"
                && matches!(
                    base_url.host_str(),
                    Some("127.0.0.1" | "localhost" | "[::1]")
                ))
        {
            anyhow::bail!("NCB_WEB_BASE_URL must use https (http is only allowed for localhost)");
        }
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || base_url.path() != "/"
        {
            anyhow::bail!(
                "NCB_WEB_BASE_URL must be an origin without credentials, path, query, or fragment"
            );
        }
        let bind = config
            .bind
            .parse()
            .context("NCB_WEB_BIND must be an IP:port socket address")?;
        if config
            .client_id
            .parse::<u64>()
            .ok()
            .filter(|id| *id != 0)
            .is_none()
        {
            anyhow::bail!("NCB_WEB_CLIENT_ID must be a Discord application ID");
        }
        if config.client_secret.trim().is_empty() {
            anyhow::bail!("NCB_WEB_CLIENT_SECRET is required when Web UI is enabled");
        }
        Ok(Self {
            bind,
            base_url,
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
        })
    }

    pub fn base_url(&self) -> &str {
        self.base_url.as_str().trim_end_matches('/')
    }

    fn redirect_uri(&self) -> String {
        format!("{}/auth/discord/callback", self.base_url())
    }

    fn secure_cookies(&self) -> bool {
        self.base_url.scheme() == "https"
    }
}

#[derive(Clone)]
struct AppState {
    config: Arc<WebConfig>,
    router: Arc<VoiceRouter>,
    cache: Arc<Cache>,
    client: Client,
    oauth_states: Arc<Mutex<HashMap<String, OAuthAttempt>>>,
    events: broadcast::Sender<Arc<SequencedEvent>>,
    event_history: Arc<Mutex<EventHistory>>,
    event_epoch: Arc<str>,
    sse_slots: Arc<Semaphore>,
}

#[derive(Clone)]
struct SequencedEvent {
    id: u64,
    sse_id: String,
    message: ServerMessage,
    encoded_size: usize,
    created_at: Instant,
}

#[derive(Default)]
struct EventHistory {
    events: VecDeque<Arc<SequencedEvent>>,
    encoded_bytes: usize,
}

struct OAuthAttempt {
    view_token: String,
    expires_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VerifiedIdentity {
    user_id: u64,
    expires_at: u64,
}

#[derive(Deserialize)]
struct OAuthCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
}

#[derive(Deserialize)]
struct DiscordUser {
    id: String,
}

#[derive(Default, Deserialize)]
struct EventStreamQuery {
    after: Option<String>,
}

pub async fn start(
    config: WebConfig,
    router: Arc<VoiceRouter>,
    cache: Arc<Cache>,
    mut result_events: broadcast::Receiver<ServerMessage>,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("failed to bind rstt Web UI to {}", config.bind))?;
    let address = listener.local_addr()?;
    let (events, _) = broadcast::channel(512);
    let event_epoch: Arc<str> = Arc::from(uuid::Uuid::new_v4().simple().to_string());
    let forward_events = events.clone();
    let forward_epoch = Arc::clone(&event_epoch);
    let event_history = Arc::new(Mutex::new(EventHistory::default()));
    let forward_history = Arc::clone(&event_history);
    let forward_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let mut next_event_id = 0_u64;
        loop {
            let result = tokio::select! { _ = forward_shutdown.cancelled() => return, result = result_events.recv() => result };
            match result {
                Ok(event) => {
                    let encoded_size =
                        serde_json::to_vec(&event).map_or(usize::MAX, |data| data.len());
                    if encoded_size > MAX_WEB_EVENT_BYTES {
                        warn!(encoded_size, "dropping oversized Web UI event");
                        continue;
                    }
                    next_event_id = next_event_id.wrapping_add(1).max(1);
                    let event = Arc::new(SequencedEvent {
                        id: next_event_id,
                        sse_id: event_cursor(&forward_epoch, next_event_id),
                        message: event,
                        encoded_size,
                        created_at: Instant::now(),
                    });
                    push_event_history(
                        &mut forward_history.lock().expect("event history poisoned"),
                        event.clone(),
                    );
                    let _ = forward_events.send(event);
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(skipped, "Web UI event forwarder lagged behind hayamimi");
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    let cleanup_shutdown = shutdown.clone();
    let cleanup_history = Arc::clone(&event_history);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! { _ = cleanup_shutdown.cancelled() => return, _ = interval.tick() => {} }
            prune_event_history(
                &mut cleanup_history.lock().expect("event history poisoned"),
                Instant::now(),
            );
        }
    });

    let base_url = config.base_url().to_owned();
    let state = AppState {
        config: Arc::new(config),
        router,
        cache,
        client: Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to create Discord OAuth HTTP client")?,
        oauth_states: Arc::new(Mutex::new(HashMap::new())),
        events,
        event_history,
        event_epoch,
        sse_slots: Arc::new(Semaphore::new(MAX_SSE_CONNECTIONS)),
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/view/{token}", get(view))
        .route("/auth/discord/callback", get(oauth_callback))
        .route("/api/events/{token}", get(event_stream))
        .route("/api/access/{token}", get(access_status))
        .route("/assets/dashboard.js", get(dashboard_js))
        .route("/assets/dashboard.css", get(dashboard_css))
        .fallback(not_found)
        .with_state(state);
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
        {
            error!(%error, "rstt Web UI server stopped");
        }
    });
    info!(%base_url, "started OAuth-protected rstt Web UI");
    Ok(address)
}

async fn healthz() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], "ok\n")
}

async fn view(
    State(app): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(grant) = app.router.web_grant(&token) else {
        return error_page(
            StatusCode::NOT_FOUND,
            "このリンクは無効または終了済みです。",
        );
    };
    if let Some(identity) = app.authorized_user(&headers) {
        if is_voice_member(&app.cache, &grant, identity.user_id) {
            return dashboard_response();
        }
        return error_page(
            StatusCode::FORBIDDEN,
            "対象のボイスチャンネルに参加している間だけ表示できます。",
        );
    }
    app.begin_oauth(token)
}

async fn oauth_callback(
    State(app): State<AppState>,
    Query(query): Query<OAuthCallback>,
    headers: HeaderMap,
) -> Response {
    if query.error.is_some() {
        return error_page(
            StatusCode::UNAUTHORIZED,
            "Discord認証がキャンセルされました。",
        );
    }
    let (Some(code), Some(state)) = (query.code.as_deref(), query.state.as_deref()) else {
        return error_page(StatusCode::BAD_REQUEST, "Discord認証応答が不完全です。");
    };
    if cookie_value(&headers, OAUTH_STATE_COOKIE).as_deref() != Some(state) {
        return error_page(
            StatusCode::UNAUTHORIZED,
            "OAuth stateを検証できませんでした。",
        );
    }
    let attempt = take_oauth_attempt(
        &mut app.oauth_states.lock().expect("OAuth state poisoned"),
        state,
    );
    let Some(attempt) = attempt else {
        return error_page(
            StatusCode::UNAUTHORIZED,
            "OAuth stateの有効期限が切れています。",
        );
    };
    let Some(grant) = app.router.web_grant(&attempt.view_token) else {
        return error_page(StatusCode::NOT_FOUND, "この通話は既に終了しています。");
    };

    let token = match app.exchange_code(code).await {
        Ok(token) => token,
        Err(error) => {
            warn!(%error, "Discord OAuth token exchange failed");
            return error_page(StatusCode::BAD_GATEWAY, "Discord認証に失敗しました。");
        }
    };
    let current_user = app.current_user(&token).await;
    app.revoke_token(&token).await;
    let user_id = match current_user {
        Ok(user_id) => user_id,
        Err(error) => {
            warn!(%error, "failed to fetch current Discord user");
            return error_page(
                StatusCode::BAD_GATEWAY,
                "Discordユーザーを取得できませんでした。",
            );
        }
    };
    if !is_voice_member(&app.cache, &grant, user_id) {
        return error_page(
            StatusCode::FORBIDDEN,
            "対象のボイスチャンネルに参加しているユーザーだけが利用できます。",
        );
    }

    let session_token = sign_identity(
        user_id,
        unix_time_seconds().saturating_add(WEB_IDENTITY_TTL.as_secs()),
        app.config.client_secret.as_bytes(),
    );
    let mut response = Redirect::to(&format!("/view/{}", attempt.view_token)).into_response();
    security_headers(response.headers_mut());
    append_set_cookie(
        &mut response,
        session_cookie(
            SESSION_COOKIE,
            &session_token,
            WEB_IDENTITY_TTL,
            "/",
            app.config.secure_cookies(),
        ),
    );
    append_set_cookie(
        &mut response,
        clear_cookie(
            OAUTH_STATE_COOKIE,
            "/auth/discord/callback",
            app.config.secure_cookies(),
        ),
    );
    response
}

async fn event_stream(
    State(app): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<EventStreamQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(grant) = app.router.web_grant(&token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(identity) = app.authorized_user(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_voice_member(&app.cache, &grant, identity.user_id) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(sse_slot) = Arc::clone(&app.sse_slots).try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };

    let requested_after = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or(query.after);
    let (receiver, pending, last_sent) = {
        let mut history = app.event_history.lock().expect("event history poisoned");
        let receiver = app.events.subscribe();
        let (pending, last_sent) = replay_events(
            &mut history,
            requested_after.as_deref(),
            &app.event_epoch,
            Instant::now(),
        );
        (receiver, pending, last_sent)
    };
    let interval = tokio::time::interval(Duration::from_secs(5));
    let stream = stream::unfold(
        (
            receiver, interval, pending, last_sent, app, grant, identity, sse_slot,
        ),
        |(
            mut receiver,
            mut interval,
            mut pending,
            mut last_sent,
            app,
            grant,
            identity,
            sse_slot,
        )| async move {
            loop {
                if let Some(sequenced) = pending.pop_front() {
                    if identity.expires_at <= unix_time_seconds()
                        || !app.router.is_web_grant_active(&grant)
                        || !is_voice_member(&app.cache, &grant, identity.user_id)
                    {
                        return None;
                    }
                    if sequenced.id <= last_sent {
                        continue;
                    }
                    last_sent = sequenced.id;
                    if let Some(data) = browser_event(&sequenced.message, &app.router, &grant) {
                        let event = Event::default()
                            .event("transcript")
                            .id(sequenced.sse_id.clone())
                            .data(data);
                        return Some((
                            Ok::<_, Infallible>(event),
                            (
                                receiver, interval, pending, last_sent, app, grant, identity,
                                sse_slot,
                            ),
                        ));
                    }
                    continue;
                }
                tokio::select! {
                    _ = interval.tick() => {
                        if identity.expires_at <= unix_time_seconds()
                            || !app.router.is_web_grant_active(&grant)
                            || !is_voice_member(&app.cache, &grant, identity.user_id)
                        {
                            return None;
                        }
                        let event = Event::default()
                            .event("keepalive")
                            .id(event_cursor(&app.event_epoch, last_sent))
                            .data("{}");
                        return Some((Ok::<_, Infallible>(event),
                            (receiver, interval, pending, last_sent, app, grant, identity, sse_slot)));
                    }
                    result = receiver.recv() => match result {
                        Ok(sequenced) => {
                            if identity.expires_at <= unix_time_seconds()
                                || !app.router.is_web_grant_active(&grant)
                                || !is_voice_member(&app.cache, &grant, identity.user_id) {
                                return None;
                            }
                            if sequenced.id <= last_sent {
                                continue;
                            }
                            last_sent = sequenced.id;
                            if let Some(data) = browser_event(&sequenced.message, &app.router, &grant) {
                                let event = Event::default()
                                    .event("transcript")
                                    .id(sequenced.sse_id.clone())
                                    .data(data);
                                return Some((Ok::<_, Infallible>(event),
                                    (receiver, interval, pending, last_sent, app, grant, identity, sse_slot)));
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!(skipped, last_sent, "Web UI SSE receiver lagged; replaying buffered events");
                            let mut history = app.event_history.lock().expect("event history poisoned");
                            let cursor = event_cursor(&app.event_epoch, last_sent);
                            (pending, _) = replay_events(
                                &mut history,
                                Some(&cursor),
                                &app.event_epoch,
                                Instant::now(),
                            );
                        }
                        Err(broadcast::error::RecvError::Closed) => return None,
                    }
                }
            }
        },
    );
    let mut response = Sse::new(stream).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

async fn access_status(
    State(app): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(grant) = app.router.web_grant(&token) else {
        return access_status_response(StatusCode::NOT_FOUND);
    };
    let Some(identity) = app.authorized_user(&headers) else {
        return access_status_response(StatusCode::UNAUTHORIZED);
    };
    if !is_voice_member(&app.cache, &grant, identity.user_id) {
        return access_status_response(StatusCode::FORBIDDEN);
    }
    access_status_response(StatusCode::NO_CONTENT)
}

fn access_status_response(status: StatusCode) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], status).into_response()
}

fn push_event_history(history: &mut EventHistory, event: Arc<SequencedEvent>) {
    prune_event_history(history, Instant::now());
    history.encoded_bytes = history.encoded_bytes.saturating_add(event.encoded_size);
    history.events.push_back(event);
    while history.events.len() > MAX_EVENT_HISTORY
        || history.encoded_bytes > MAX_EVENT_HISTORY_BYTES
    {
        let Some(removed) = history.events.pop_front() else {
            break;
        };
        history.encoded_bytes = history.encoded_bytes.saturating_sub(removed.encoded_size);
    }
}

fn prune_event_history(history: &mut EventHistory, now: Instant) {
    while history
        .events
        .front()
        .is_some_and(|event| now.duration_since(event.created_at) >= EVENT_HISTORY_TTL)
    {
        let removed = history.events.pop_front().expect("front event exists");
        history.encoded_bytes = history.encoded_bytes.saturating_sub(removed.encoded_size);
    }
}

fn replay_events(
    history: &mut EventHistory,
    requested_after: Option<&str>,
    event_epoch: &str,
    now: Instant,
) -> (VecDeque<Arc<SequencedEvent>>, u64) {
    prune_event_history(history, now);
    let latest = history.events.back().map_or(0, |event| event.id);
    let Some(requested_after) = requested_after else {
        return (VecDeque::new(), latest);
    };
    let after = parse_event_cursor(requested_after)
        .map(|(epoch, sequence)| {
            if epoch == event_epoch {
                sequence.min(latest)
            } else {
                0
            }
        })
        .unwrap_or(latest);
    (
        history
            .events
            .iter()
            .filter(|event| event.id > after)
            .cloned()
            .collect(),
        after,
    )
}

fn event_cursor(epoch: &str, sequence: u64) -> String {
    format!("{epoch}:{sequence}")
}

fn parse_event_cursor(cursor: &str) -> Option<(&str, u64)> {
    let (epoch, sequence) = cursor.split_once(':')?;
    if epoch.len() != 32 || !epoch.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some((epoch, sequence.parse().ok()?))
}

impl AppState {
    fn begin_oauth(&self, view_token: String) -> Response {
        let state = random_token();
        let expires_at = Instant::now() + OAUTH_STATE_TTL;
        insert_oauth_attempt(
            &mut self.oauth_states.lock().expect("OAuth state poisoned"),
            state.clone(),
            OAuthAttempt {
                view_token,
                expires_at,
            },
        );

        let mut authorize = Url::parse(DISCORD_AUTHORIZE_URL).expect("Discord URL is valid");
        authorize.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", self.config.client_id.as_str()),
            ("scope", "identify"),
            ("state", state.as_str()),
            ("redirect_uri", self.config.redirect_uri().as_str()),
        ]);
        let mut response = Redirect::temporary(authorize.as_str()).into_response();
        security_headers(response.headers_mut());
        append_set_cookie(
            &mut response,
            session_cookie(
                OAUTH_STATE_COOKIE,
                &state,
                OAUTH_STATE_TTL,
                "/auth/discord/callback",
                self.config.secure_cookies(),
            ),
        );
        response
    }

    fn authorized_user(&self, headers: &HeaderMap) -> Option<VerifiedIdentity> {
        let token = cookie_value(headers, SESSION_COOKIE)?;
        verify_identity(
            &token,
            self.config.client_secret.as_bytes(),
            unix_time_seconds(),
        )
    }

    async fn exchange_code(&self, code: &str) -> anyhow::Result<TokenResponse> {
        self.client
            .post(DISCORD_TOKEN_URL)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.config.redirect_uri().as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("invalid Discord token response")
    }

    async fn current_user(&self, token: &TokenResponse) -> anyhow::Result<u64> {
        let user: DiscordUser = self
            .client
            .get(DISCORD_CURRENT_USER_URL)
            .header(
                header::AUTHORIZATION,
                format!("{} {}", token.token_type, token.access_token),
            )
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        user.id
            .parse()
            .context("Discord returned an invalid user id")
    }

    async fn revoke_token(&self, token: &TokenResponse) {
        let result = self
            .client
            .post(DISCORD_REVOKE_URL)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("token", token.access_token.as_str()),
            ])
            .send()
            .await
            .and_then(reqwest::Response::error_for_status);
        if let Err(error) = result {
            warn!(%error, "failed to revoke short-lived Discord OAuth token");
        }
    }
}

fn insert_oauth_attempt(
    attempts: &mut HashMap<String, OAuthAttempt>,
    token: String,
    attempt: OAuthAttempt,
) {
    let now = Instant::now();
    attempts.retain(|_, attempt| attempt.expires_at > now);
    evict_earliest(attempts, MAX_OAUTH_STATES, |attempt| attempt.expires_at);
    attempts.insert(token, attempt);
}

fn take_oauth_attempt(
    attempts: &mut HashMap<String, OAuthAttempt>,
    token: &str,
) -> Option<OAuthAttempt> {
    let now = Instant::now();
    attempts.retain(|_, attempt| attempt.expires_at > now);
    attempts.remove(token)
}

fn sign_identity(user_id: u64, expires_at: u64, secret: &[u8]) -> String {
    let payload = format!("v1.{user_id}.{expires_at}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepts any key size");
    mac.update(SESSION_SIGNATURE_DOMAIN);
    mac.update(payload.as_bytes());
    format!(
        "{payload}.{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

fn verify_identity(token: &str, secret: &[u8], now: u64) -> Option<VerifiedIdentity> {
    if token.len() > MAX_SESSION_TOKEN_LEN {
        return None;
    }
    let (payload, signature) = token.rsplit_once('.')?;
    let mut fields = payload.split('.');
    if fields.next()? != "v1" {
        return None;
    }
    let user_id = fields.next()?.parse().ok()?;
    let expires_at: u64 = fields.next()?.parse().ok()?;
    if fields.next().is_some() || expires_at <= now {
        return None;
    }
    let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
    if signature.len() != 32 {
        return None;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    mac.update(SESSION_SIGNATURE_DOMAIN);
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature).ok()?;
    Some(VerifiedIdentity {
        user_id,
        expires_at,
    })
}

fn unix_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn evict_earliest<T>(
    values: &mut HashMap<String, T>,
    capacity: usize,
    expires_at: impl Fn(&T) -> Instant,
) {
    if values.len() < capacity {
        return;
    }
    if let Some(oldest) = values
        .iter()
        .min_by_key(|(_, value)| expires_at(value))
        .map(|(key, _)| key.clone())
    {
        values.remove(&oldest);
    }
}

fn is_voice_member(cache: &Cache, grant: &WebGrant, user_id: u64) -> bool {
    cache
        .guild(SerenityGuildId::new(grant.guild_id.0))
        .and_then(|guild| {
            guild
                .voice_states
                .get(&SerenityUserId::new(user_id))
                .and_then(|state| state.channel_id)
        })
        .is_some_and(|channel_id| channel_id.get() == grant.voice_channel_id)
}

fn browser_event(
    message: &ServerMessage,
    router: &VoiceRouter,
    grant: &WebGrant,
) -> Option<String> {
    if !matches!(message.kind.as_str(), "partial" | "final" | "translation") {
        return None;
    }
    let stream_id = message
        .fields
        .get("stream_id")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())?;
    let utterance_id = message.fields.get("utterance_id")?.as_str()?;
    let text = message.fields.get("text")?.as_str()?;
    let context = router.web_event_context(grant, stream_id)?;
    let lang = message
        .fields
        .get("lang")
        .and_then(Value::as_str)
        .filter(|lang| matches!(*lang, "ja" | "en" | "ko"));
    if message.kind == "translation" && lang.is_none() {
        return None;
    }
    let source_lang = message
        .fields
        .get("source_lang")
        .and_then(Value::as_str)
        .filter(|lang| matches!(*lang, "ja" | "en" | "ko"));

    // Build an allowlisted browser payload. Identity and routing fields are
    // sourced only from the context captured when this Discord stream opened.
    let mut event = serde_json::Map::new();
    event.insert("type".to_owned(), Value::String(message.kind.clone()));
    event.insert("stream_id".to_owned(), Value::from(stream_id));
    event.insert(
        "utterance_id".to_owned(),
        Value::String(utterance_id.to_owned()),
    );
    event.insert("text".to_owned(), Value::String(text.to_owned()));
    event.insert("speaker".to_owned(), Value::String(context.speaker));
    if let Some(avatar_url) = context.avatar_url {
        event.insert("avatar_url".to_owned(), Value::String(avatar_url));
    }
    if let Some(lang) = lang {
        event.insert("lang".to_owned(), Value::String(lang.to_owned()));
    }
    if let Some(source_lang) = source_lang {
        event.insert(
            "source_lang".to_owned(),
            Value::String(source_lang.to_owned()),
        );
    }
    serde_json::to_string(&Value::Object(event)).ok()
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

fn session_cookie(name: &str, value: &str, ttl: Duration, path: &str, secure: bool) -> String {
    format!(
        "{name}={value}; Path={path}; Max-Age={}; HttpOnly; SameSite=Lax{}",
        ttl.as_secs(),
        if secure { "; Secure" } else { "" }
    )
}

fn clear_cookie(name: &str, path: &str, secure: bool) -> String {
    format!(
        "{name}=; Path={path}; Max-Age=0; HttpOnly; SameSite=Lax{}",
        if secure { "; Secure" } else { "" }
    )
}

fn append_set_cookie(response: &mut Response, cookie: String) {
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn dashboard_response() -> Response {
    let mut response = Html(DASHBOARD_HTML).into_response();
    security_headers(response.headers_mut());
    response
}

async fn dashboard_js() -> Response {
    asset_response("text/javascript; charset=utf-8", DASHBOARD_JS)
}

async fn dashboard_css() -> Response {
    asset_response("text/css; charset=utf-8", DASHBOARD_CSS)
}

fn asset_response(content_type: &'static str, body: &'static str) -> Response {
    let mut response = body.into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn error_page(status: StatusCode, message: &'static str) -> Response {
    let body = format!(
        "<!doctype html><html lang=\"ja\"><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width,initial-scale=1\"><title>rstt</title><body><main><h1>rstt</h1>\
         <p>{message}</p></main></body></html>"
    );
    let mut response = (status, Html(body)).into_response();
    security_headers(response.headers_mut());
    response
}

fn security_headers(headers: &mut HeaderMap) {
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' https://cdn.discordapp.com; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
}

async fn not_found() -> Response {
    error_page(StatusCode::NOT_FOUND, "ページが見つかりません。")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::{
        bridge::AudioBridge,
        protocol::{AudioFrame, StreamOpen},
        router::{GuildId, UserId},
    };
    use serde_json::json;

    struct NoopBridge;

    const TEST_EVENT_EPOCH: &str = "0123456789abcdef0123456789abcdef";

    impl AudioBridge for NoopBridge {
        fn open(&self, _stream: StreamOpen) {}
        fn audio(&self, _frame: AudioFrame) -> bool {
            true
        }
        fn idle(&self, _stream_id: u32) {}
        fn gap(&self, _stream_id: u32, _reason: &str) {}
        fn end(&self, _stream_id: u32, _reason: &str) {}
    }

    #[test]
    fn cookies_are_scoped_and_hardened() {
        let cookie = session_cookie("session", "opaque", Duration::from_secs(60), "/", true);
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("; Secure"));
        assert!(cookie.contains("Max-Age=60"));
    }

    #[test]
    fn old_stream_cannot_cross_into_a_restarted_voice_session() {
        let router = VoiceRouter::new(Arc::new(NoopBridge), false);
        let old_token = router.start_guild(GuildId(123), 789);
        let old_grant = router.web_grant(&old_token).unwrap();
        router.speaking_state_with_avatar(GuildId(123), 42, UserId(55), "Alice".to_owned(), None);
        let event: ServerMessage = serde_json::from_value(json!({
            "type": "final",
            "stream_id": 1,
            "utterance_id": "1-1",
            "metadata": {"guild_id": "123", "channel_id": "789"},
            "text": "hello"
        }))
        .unwrap();
        assert!(browser_event(&event, &router, &old_grant).is_some());

        router.stop_guild(GuildId(123), "test");
        let new_token = router.start_guild(GuildId(123), 789);
        let new_grant = router.web_grant(&new_token).unwrap();
        assert!(browser_event(&event, &router, &new_grant).is_none());
    }

    #[test]
    fn dashboard_uses_external_assets_for_strict_csp() {
        assert!(DASHBOARD_HTML.contains("/assets/dashboard.js"));
        assert!(!DASHBOARD_HTML.contains("<script>"));
        assert!(!DASHBOARD_HTML.contains("音声データは保存されません"));
    }

    #[test]
    fn browser_event_uses_only_the_discord_avatar() {
        let router = VoiceRouter::new(Arc::new(NoopBridge), false);
        router.start_guild(GuildId(123), 789);
        router.speaking_state_with_avatar(
            GuildId(123),
            42,
            UserId(55),
            "Alice".to_owned(),
            Some("https://cdn.discordapp.com/avatars/55/discord.webp".to_owned()),
        );
        let event: ServerMessage = serde_json::from_value(json!({
            "type": "final",
            "stream_id": 1,
            "utterance_id": "1-1",
            "speaker_id": "55",
            "speaker": "Mallory",
            "avatar_url": "https://attacker.invalid/tracker.png",
            "metadata": {"guild_id": "123", "channel_id": "789"},
            "text": "hello",
            "internal_diagnostic": "must not leak"
        }))
        .unwrap();
        let grant = WebGrant {
            guild_id: GuildId(123),
            voice_channel_id: 789,
            token: router.web_token(GuildId(123)).unwrap(),
        };

        let rendered: Value =
            serde_json::from_str(&browser_event(&event, &router, &grant).unwrap()).unwrap();
        assert_eq!(
            rendered["avatar_url"],
            "https://cdn.discordapp.com/avatars/55/discord.webp"
        );
        assert_eq!(rendered["speaker"], "Alice");
        assert!(rendered.get("internal_diagnostic").is_none());
    }

    #[test]
    fn oauth_state_is_bounded_expiring_and_one_time() {
        let mut attempts = HashMap::new();
        insert_oauth_attempt(
            &mut attempts,
            "expired".to_owned(),
            OAuthAttempt {
                view_token: "old".to_owned(),
                expires_at: Instant::now() - Duration::from_secs(1),
            },
        );
        for index in 0..=MAX_OAUTH_STATES {
            insert_oauth_attempt(
                &mut attempts,
                format!("state-{index}"),
                OAuthAttempt {
                    view_token: format!("view-{index}"),
                    expires_at: Instant::now() + Duration::from_secs(60 + index as u64),
                },
            );
        }
        assert_eq!(attempts.len(), MAX_OAUTH_STATES);
        assert!(!attempts.contains_key("expired"));
        let state = format!("state-{MAX_OAUTH_STATES}");
        assert!(take_oauth_attempt(&mut attempts, &state).is_some());
        assert!(take_oauth_attempt(&mut attempts, &state).is_none());
    }

    #[test]
    fn signed_identity_survives_restart_but_rejects_expiry_and_tampering() {
        let secret = b"discord-client-secret";
        let token = sign_identity(55, 1_000, secret);
        assert_eq!(
            verify_identity(&token, secret, 999),
            Some(VerifiedIdentity {
                user_id: 55,
                expires_at: 1_000,
            })
        );
        assert_eq!(verify_identity(&token, secret, 1_000), None);
        assert_eq!(verify_identity(&token, b"rotated-secret", 999), None);

        let mut tampered = token.into_bytes();
        *tampered.last_mut().unwrap() ^= 1;
        assert_eq!(
            verify_identity(std::str::from_utf8(&tampered).unwrap(), secret, 999),
            None
        );

        assert_eq!(
            verify_identity(&"x".repeat(MAX_SESSION_TOKEN_LEN + 1), secret, 999),
            None
        );
        let short_signature = format!("v1.55.1000.{}", URL_SAFE_NO_PAD.encode([0_u8; 31]));
        assert_eq!(verify_identity(&short_signature, secret, 999), None);
    }

    fn sequenced_event(id: u64) -> Arc<SequencedEvent> {
        Arc::new(SequencedEvent {
            id,
            sse_id: event_cursor(TEST_EVENT_EPOCH, id),
            message: serde_json::from_value(json!({
                "type": "final",
                "stream_id": 1,
                "utterance_id": format!("1-{id}"),
                "text": format!("event-{id}")
            }))
            .unwrap(),
            encoded_size: 64,
            created_at: Instant::now(),
        })
    }

    #[test]
    fn event_history_is_bounded_and_replays_after_cursor() {
        let mut history = EventHistory::default();
        for id in 1..=(MAX_EVENT_HISTORY as u64 + 1) {
            push_event_history(&mut history, sequenced_event(id));
        }
        assert_eq!(history.events.len(), MAX_EVENT_HISTORY);
        assert_eq!(history.events.front().unwrap().id, 2);

        let expected_cursor = MAX_EVENT_HISTORY as u64 - 1;
        let cursor = event_cursor(TEST_EVENT_EPOCH, expected_cursor);
        let (replayed, last_sent) = replay_events(
            &mut history,
            Some(&cursor),
            TEST_EVENT_EPOCH,
            Instant::now(),
        );
        assert_eq!(last_sent, expected_cursor);
        assert_eq!(
            replayed.iter().map(|event| event.id).collect::<Vec<_>>(),
            vec![MAX_EVENT_HISTORY as u64, MAX_EVENT_HISTORY as u64 + 1]
        );
    }

    #[test]
    fn fresh_sse_starts_at_latest_event_and_future_cursor_is_clamped() {
        let mut history = EventHistory::default();
        push_event_history(&mut history, sequenced_event(7));
        push_event_history(&mut history, sequenced_event(8));
        let (fresh, fresh_cursor) =
            replay_events(&mut history, None, TEST_EVENT_EPOCH, Instant::now());
        assert!(fresh.is_empty());
        assert_eq!(fresh_cursor, 8);

        let future_id = event_cursor(TEST_EVENT_EPOCH, u64::MAX);
        let (future, future_cursor) = replay_events(
            &mut history,
            Some(&future_id),
            TEST_EVENT_EPOCH,
            Instant::now(),
        );
        assert!(future.is_empty());
        assert_eq!(future_cursor, 8);

        let previous_epoch = "fedcba9876543210fedcba9876543210:99";
        let (restarted, restart_cursor) = replay_events(
            &mut history,
            Some(previous_epoch),
            TEST_EVENT_EPOCH,
            Instant::now(),
        );
        assert_eq!(restart_cursor, 0);
        assert_eq!(
            restarted.iter().map(|event| event.id).collect::<Vec<_>>(),
            vec![7, 8]
        );
    }

    #[test]
    fn event_history_expires_and_tracks_its_byte_budget() {
        let now = Instant::now();
        let mut expired = sequenced_event(1);
        Arc::get_mut(&mut expired).unwrap().created_at = now - EVENT_HISTORY_TTL;
        let mut history = EventHistory::default();
        push_event_history(&mut history, expired);
        prune_event_history(&mut history, now);
        assert!(history.events.is_empty());
        assert_eq!(history.encoded_bytes, 0);

        let mut large = sequenced_event(2);
        Arc::get_mut(&mut large).unwrap().encoded_size = MAX_EVENT_HISTORY_BYTES;
        push_event_history(&mut history, large);
        push_event_history(&mut history, sequenced_event(3));
        assert_eq!(history.events.len(), 1);
        assert_eq!(history.events.front().unwrap().id, 3);
        assert_eq!(history.encoded_bytes, 64);
    }

    fn voice_state(cache: &Cache, channel: Option<&str>) {
        let mut event: serenity::all::VoiceStateUpdateEvent = serde_json::from_value(json!({
            "guild_id": "123", "channel_id": channel, "user_id": "55", "session_id": "test",
            "deaf": false, "mute": false, "self_deaf": false, "self_mute": false,
            "self_video": false, "suppress": false
        }))
        .unwrap();
        cache.update(&mut event);
    }

    async fn read_through(response: &mut reqwest::Response, expected: &str) -> String {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut text = String::new();
            loop {
                let chunk = response
                    .chunk()
                    .await
                    .unwrap()
                    .expect("SSE ended before expected event");
                text.push_str(std::str::from_utf8(&chunk).unwrap());
                if text.contains(expected) {
                    return text;
                }
            }
        })
        .await
        .expect("SSE event deadline")
    }

    #[tokio::test]
    async fn http_auth_sse_translation_replay_and_voice_revocation() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let router = Arc::new(VoiceRouter::new(Arc::new(NoopBridge), false));
        let token = router.start_guild(GuildId(123), 789);
        router.speaking_state_with_avatar(GuildId(123), 42, UserId(55), "Alice".into(), None);
        let cache = Arc::new(Cache::new());
        let mut guild = serenity::all::Guild::default();
        guild.id = SerenityGuildId::new(123);
        let mut create: serenity::all::GuildCreateEvent =
            serde_json::from_value(serde_json::to_value(guild).unwrap()).unwrap();
        cache.update(&mut create);
        voice_state(&cache, Some("789"));
        let (results, events) = broadcast::channel(32);
        let shutdown = tokio_util::sync::CancellationToken::new();
        let address = start(
            WebConfig::new(&crate::config::WebConfig {
                bind: "127.0.0.1:0".into(),
                base_url: "http://localhost".into(),
                client_id: "42".into(),
                client_secret: "test-secret".into(),
                enabled: true,
            })
            .unwrap(),
            router.clone(),
            cache.clone(),
            events,
            shutdown.clone(),
        )
        .await
        .unwrap();
        let base = format!("http://{address}");
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let identity = sign_identity(55, unix_time_seconds() + 60, b"test-secret");
        let cookie = format!("{SESSION_COOKIE}={identity}");
        assert_eq!(
            client
                .get(format!("{base}/healthz"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "ok\n"
        );
        let login = client
            .get(format!("{base}/view/{token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::TEMPORARY_REDIRECT);
        assert!(login.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .starts_with(DISCORD_AUTHORIZE_URL));
        assert!(login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("HttpOnly"));
        assert_eq!(
            client
                .get(format!("{base}/api/events/{token}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .get(format!("{base}/auth/discord/callback?code=fake&state=fake"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let page = client
            .get(format!("{base}/view/{token}"))
            .header(header::COOKIE, &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        assert!(page.headers().contains_key("content-security-policy"));
        assert!(page.text().await.unwrap().contains("ライブ文字起こし"));
        let asset = client
            .get(format!("{base}/assets/dashboard.js"))
            .send()
            .await
            .unwrap();
        assert_eq!(asset.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(asset.text().await.unwrap().contains("EventSource"));
        let mut live = client
            .get(format!("{base}/api/events/{token}"))
            .header(header::COOKIE, &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(live.status(), StatusCode::OK);
        assert_eq!(live.headers()["x-accel-buffering"], "no");
        for (kind, lang, text) in [
            ("partial", "ja", "こん"),
            ("final", "ja", "こんにちは"),
            ("translation", "en", "hello"),
            ("translation", "ko", "안녕하세요"),
        ] {
            results.send(serde_json::from_value(json!({"type": kind, "stream_id": 1, "utterance_id": "1-1", "lang": lang, "text": text, "speaker": "forged"})).unwrap()).unwrap();
            let delivered = read_through(&mut live, text).await;
            assert!(delivered.contains("Alice"));
            assert!(!delivered.contains("forged"));
        }
        // Reconnect after the final: both translation events must be replayed.
        let epoch = "00000000000000000000000000000000:0";
        let mut replay = client
            .get(format!("{base}/api/events/{token}?after={epoch}"))
            .header(header::COOKIE, &cookie)
            .send()
            .await
            .unwrap();
        let replayed = read_through(&mut replay, "안녕하세요").await;
        assert!(replayed.contains("こんにちは") && replayed.contains("hello"));
        voice_state(&cache, None);
        assert_eq!(
            client
                .get(format!("{base}/api/access/{token}"))
                .header(header::COOKIE, &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        results.send(serde_json::from_value(json!({"type": "final", "stream_id": 1, "utterance_id": "1-2", "text": "must-not-leak"})).unwrap()).unwrap();
        let remainder = tokio::time::timeout(Duration::from_secs(6), live.text())
            .await
            .unwrap()
            .unwrap();
        assert!(!remainder.contains("must-not-leak"));
        voice_state(&cache, Some("789"));
        assert_eq!(
            client
                .get(format!("{base}/api/access/{token}"))
                .header(header::COOKIE, &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NO_CONTENT
        );
        let grant = router.stop_guild(GuildId(123), "test").unwrap();
        router.finish_web_drain(&grant);
        assert_eq!(
            client
                .get(format!("{base}/view/{token}"))
                .header(header::COOKIE, &cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        shutdown.cancel();
    }
}
