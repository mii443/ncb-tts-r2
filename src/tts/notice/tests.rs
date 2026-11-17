use super::*;
use serde_json::{json, Value};
use serenity::http::HttpBuilder;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

struct Server {
    http: Http,
    requests: Arc<Mutex<Vec<Value>>>,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(statuses: &[(u16, u32)]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let statuses = statuses.to_vec();
        let task = tokio::spawn(async move {
            for (status, code) in statuses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let read = stream.read(&mut buffer).await.unwrap();
                    assert_ne!(read, 0);
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        assert!(headers.starts_with("POST "));
                        assert!(headers.contains("/channels/123/messages "));
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if request.len() >= end + 4 + length {
                            captured.lock().unwrap().push(
                                serde_json::from_slice(&request[end + 4..end + 4 + length])
                                    .unwrap(),
                            );
                            break;
                        }
                    }
                }
                let body = if status == 200 {
                    serde_json::to_vec(&serenity::all::Message::default()).unwrap()
                } else {
                    serde_json::to_vec(&json!({"code":code,"message":"Missing Permissions"}))
                        .unwrap()
                };
                let headers = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            }
        });
        let http = HttpBuilder::without_token()
            .proxy(url)
            .ratelimiter_disabled(true)
            .build();
        Self {
            http,
            requests,
            task,
        }
    }

    async fn send(&self, permissions: Option<Permissions>) -> serenity::Result<()> {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            send_credits(
                &self.http,
                ChannelId::new(123),
                "読み上げ",
                &["ずんだもん".into(), "四国めたん".into()],
                permissions,
            ),
        )
        .await
        .expect("notification timed out")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn assert_plain_text(request: &Value) {
    assert!(request
        .get("embeds")
        .is_none_or(|v| v.as_array().is_some_and(|a| a.is_empty())));
    let content = request["content"].as_str().unwrap();
    assert!(content.contains("ずんだもん"));
    assert!(content.contains("四国めたん"));
    assert!(content.contains("/config"));
    assert_eq!(request["allowed_mentions"]["parse"], json!([]));
    assert_eq!(request["flags"], MessageFlags::SUPPRESS_EMBEDS.bits());
}

#[tokio::test]
async fn missing_embed_and_history_permissions_send_plain_text_directly() {
    let server = Server::start(&[(200, 0)]).await;
    server
        .send(Some(
            Permissions::VIEW_CHANNEL
                | Permissions::SEND_MESSAGES
                | Permissions::CONNECT
                | Permissions::SPEAK,
        ))
        .await
        .unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_plain_text(&requests[0]);
}

#[tokio::test]
async fn denied_embed_retries_with_plain_text_for_changed_or_unknown_permissions() {
    for permissions in [Some(Permissions::EMBED_LINKS), None] {
        let server = Server::start(&[(403, 50013), (200, 0)]).await;
        server.send(permissions).await.unwrap();
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["embeds"][0]["title"], "読み上げ");
        assert_plain_text(&requests[1]);
    }
}

#[tokio::test]
async fn permitted_embed_is_sent_once() {
    let server = Server::start(&[(200, 0)]).await;
    server.send(Some(Permissions::EMBED_LINKS)).await.unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0]["embeds"][0]["fields"][0]["value"]
        .as_str()
        .unwrap()
        .contains("ずんだもん"));
}

#[tokio::test]
async fn missing_send_permission_stops_after_one_plain_text_fallback() {
    let server = Server::start(&[(403, 50013), (403, 50013)]).await;
    assert!(server.send(None).await.is_err());
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_plain_text(&requests[1]);
}

#[tokio::test]
async fn missing_access_is_not_retried_as_an_embed_error() {
    let server = Server::start(&[(403, 50001)]).await;
    assert!(server.send(None).await.is_err());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn long_credit_lists_preserve_all_names_within_message_limits() {
    let server = Server::start(&[(200, 0), (200, 0)]).await;
    let name = "あ".repeat(2200);
    send_credits(
        &server.http,
        ChannelId::new(123),
        "読み上げ",
        std::slice::from_ref(&name),
        Some(Permissions::EMBED_LINKS),
    )
    .await
    .unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let contents: Vec<_> = requests
        .iter()
        .map(|r| r["content"].as_str().unwrap())
        .collect();
    assert!(contents.iter().all(|s| s.chars().count() <= 2000));
    assert!(contents.concat().contains(&name));
}
