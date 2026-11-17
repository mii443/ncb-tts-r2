use super::{gcp_tts::gcp_tts::GCPTTS, http, tts::TTS, voicevox::voicevox::VOICEVOX};
use crate::{errors::NCBError, stream_input::Mp3Request};
use base64::{engine::general_purpose::STANDARD, Engine};
use songbird::input::{
    codecs::{get_codec_registry, get_probe},
    Compose,
};
use std::{
    io::Write,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

// These tests use only loopback HTTP and generated silent audio, never credentials
// or third-party services. Dropping a server also cancels a deliberately hung reply.
struct Server {
    url: String,
    requests: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(responses: Vec<Option<(u16, Vec<u8>)>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let task = tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let n = stream.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..n]);
                    if let Some(end) = request.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                count.fetch_add(1, Ordering::SeqCst);
                let Some((status, body)) = response else {
                    std::future::pending::<()>().await;
                    return;
                };
                let header = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if stream.write_all(header.as_bytes()).await.is_err() {
                    return;
                }
                if stream.write_all(&body).await.is_err() {
                    return;
                }
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }

    fn count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn reply(status: u16, body: impl Into<Vec<u8>>) -> Option<(u16, Vec<u8>)> {
    Some((status, body.into()))
}

fn request() -> super::gcp_tts::structs::synthesize_request::SynthesizeRequest {
    serde_json::from_value(serde_json::json!({
        "input": {"text": "private-message-text"},
        "voice": {"languageCode": "ja-JP", "name": "ja-JP-Wavenet-B", "ssmlGender": "neutral"},
        "audioConfig": {"audioEncoding": "mp3", "speakingRate": 1.2, "pitch": 1.0}
    }))
    .unwrap()
}

fn silent_mp3() -> Vec<u8> {
    let mut builder = mp3lame_encoder::Builder::new().unwrap();
    builder.set_sample_rate(48000).unwrap();
    builder.set_num_channels(1).unwrap();
    let mut encoder = builder.build().unwrap();
    let mut audio = Vec::with_capacity(32768);
    encoder
        .encode_to_vec(mp3lame_encoder::MonoPcm(&[0i16; 4800]), &mut audio)
        .unwrap();
    encoder
        .flush_to_vec::<mp3lame_encoder::FlushGap>(&mut audio)
        .unwrap();
    audio
}

#[tokio::test]
async fn gcp_status_and_malformed_responses_return_errors_without_panicking() {
    for (status, body) in [
        (400, "secret-error-body"),
        (200, "invalid-json"),
        (200, "{}"),
        (200, r#"{"audioContent":"not base64!"}"#),
    ] {
        let server = Server::start(vec![reply(status, body)]).await;
        let client = GCPTTS::for_test(server.url.clone());
        let error = client.synthesize(request()).await.unwrap_err();
        assert!(!error.is_retryable());
        assert!(!format!("{error:?} {error}").contains("secret-error-body"));
        assert_eq!(server.count(), 1);
    }
}

#[tokio::test]
async fn gcp_service_retries_at_most_three_times_and_never_retries_bad_requests() {
    for (status, expected) in [(503, 3), (429, 3), (400, 1), (401, 1)] {
        let server = Server::start(vec![
            reply(status, "{}"),
            reply(status, "{}"),
            reply(status, "{}"),
        ])
        .await;
        let tts = TTS::new(
            VOICEVOX::for_test(server.url.clone(), None),
            GCPTTS::for_test(server.url.clone()),
        )
        .with_cache_path(None);
        assert!(tts.synthesize_gcp(request()).await.is_err());
        assert_eq!(server.count(), expected);
    }
}

#[tokio::test]
async fn gcp_transient_failure_recovers_to_playable_audio() {
    let audio = silent_mp3();
    let body = serde_json::json!({"audioContent": STANDARD.encode(audio)}).to_string();
    let server = Server::start(vec![reply(503, "{}"), reply(200, body)]).await;
    let tts = TTS::new(
        VOICEVOX::for_test(server.url.clone(), None),
        GCPTTS::for_test(server.url.clone()),
    )
    .with_cache_path(None);
    let track = tts.synthesize_gcp(request()).await.unwrap();
    let input = track
        .input
        .make_playable_async(get_codec_registry(), get_probe())
        .await
        .unwrap();
    assert!(input.is_playable());
    assert_eq!(server.count(), 2);
}

#[tokio::test(start_paused = true)]
async fn retry_budget_includes_a_hung_request() {
    let started = tokio::time::Instant::now();
    let result = http::retry_synthesis(std::future::pending::<crate::errors::Result<()>>).await;
    assert!(matches!(result, Err(NCBError::Timeout { .. })));
    assert_eq!(
        started.elapsed(),
        Duration::from_secs(crate::errors::constants::TTS_TIMEOUT_SECS)
    );
}

#[tokio::test]
async fn http_client_enforces_a_total_response_timeout() {
    let server = Server::start(vec![None]).await;
    // Override with a short deadline to exercise reqwest's real socket/body path.
    let error: NCBError = http::client()
        .unwrap()
        .get(format!("{}/?key=test-secret-key", server.url))
        .timeout(Duration::from_millis(30))
        .send()
        .await
        .unwrap_err()
        .into();
    assert!(error.is_retryable());
    assert!(!format!("{error:?} {error}").contains("test-secret-key"));
}

#[tokio::test]
async fn voicevox_key_stream_is_returned_as_a_playable_track() {
    let audio = Server::start(vec![reply(200, silent_mp3())]).await;
    let response = serde_json::json!({"success": true, "isApiKeyValid": true, "mp3StreamingUrl": format!("{}/audio?key=signed-secret", audio.url)}).to_string();
    let api = Server::start(vec![reply(200, response)]).await;
    let tts = TTS::new(
        VOICEVOX::for_test(api.url.clone(), None),
        GCPTTS::for_test(api.url.clone()),
    )
    .with_cache_path(None);
    let track = tts.synthesize_voicevox("hello", 1).await.unwrap();
    let input = track
        .input
        .make_playable_async(get_codec_registry(), get_probe())
        .await
        .unwrap();
    assert!(input.is_playable());
    assert_eq!(api.count(), 1);
    assert_eq!(audio.count(), 1);
}

#[tokio::test]
async fn voicevox_invalid_streams_and_failed_downloads_are_errors() {
    for body in [
        "{}",
        "invalid-json",
        r#"{"success":false}"#,
        r#"{"success":true,"isApiKeyValid":false}"#,
        r#"{"success":true}"#,
        r#"{"success":true,"mp3StreamingUrl":"file:///etc/passwd"}"#,
    ] {
        let server = Server::start(vec![reply(200, body)]).await;
        assert!(VOICEVOX::for_test(server.url.clone(), None)
            .synthesize_stream("hello".into(), 1)
            .await
            .is_err());
    }
    let server = Server::start(vec![reply(503, "test-secret-body")]).await;
    let mut request = Mp3Request::new(
        http::client().unwrap(),
        format!("{}?key=test-secret-key", server.url),
    );
    let error = request.create_async().await.err().unwrap();
    assert!(!format!("{error:?} {error}").contains("test-secret"));
}

#[tokio::test]
async fn voicevox_original_engine_checks_both_http_responses() {
    for responses in [
        vec![reply(400, "{}")],
        vec![reply(200, "{}"), reply(503, "error")],
    ] {
        let expected = responses.len();
        let server = Server::start(responses).await;
        let client = VOICEVOX::for_test(server.url.clone(), Some(server.url.clone()));
        assert!(client.synthesize_original("hello".into(), 1).await.is_err());
        assert_eq!(server.count(), expected);
    }
    let audio = silent_mp3();
    let server = Server::start(vec![reply(200, "{}"), reply(200, audio.clone())]).await;
    let client = VOICEVOX::for_test(server.url.clone(), Some(server.url.clone()));
    assert_eq!(
        client.synthesize_original("hello".into(), 1).await.unwrap(),
        audio
    );
}

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);
impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn debug_and_instrumentation_do_not_record_secrets_or_text() {
    use tracing::instrument::WithSubscriber;
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NEW)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let server = Server::start(vec![
        reply(400, "private-message-text"),
        reply(400, "test-secret-key"),
    ])
    .await;
    let gcp = GCPTTS::for_test(server.url.clone());
    let voicevox = VOICEVOX::for_test(server.url.clone(), None);
    let debug = format!(
        "{gcp:?} {voicevox:?} {:?}",
        Mp3Request::new(
            http::client().unwrap(),
            "https://example.invalid/?key=test-secret-key".into()
        )
    );
    async {
        let error = gcp.synthesize(request()).await.unwrap_err();
        tracing::warn!(error = %error, "GCP failed");
        let error = voicevox
            .synthesize_stream("private-message-text".into(), 1)
            .await
            .unwrap_err();
        tracing::warn!(error = %error, "VOICEVOX failed");
    }
    .with_subscriber(subscriber)
    .await;
    let output = format!(
        "{debug} {}",
        String::from_utf8(logs.0.lock().unwrap().clone()).unwrap()
    );
    assert!(output.contains("synthesize"));
    for secret in [
        "test-secret-token",
        "test-secret-key",
        "private-message-text",
    ] {
        assert!(!output.contains(secret));
    }
}
