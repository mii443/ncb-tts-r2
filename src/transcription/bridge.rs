use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};
use uuid::Uuid;

use crate::transcription::protocol::{AudioFrame, Hello, ServerMessage, StreamControl, StreamOpen};

#[derive(Clone, Debug)]
pub enum BridgeCommand {
    Open(StreamOpen),
    Idle(u32),
    Gap(u32, String),
    End(u32, String),
}

#[async_trait]
pub trait AudioBridge: Send + Sync {
    fn open(&self, stream: StreamOpen);
    fn audio(&self, frame: AudioFrame) -> bool;
    fn idle(&self, stream_id: u32);
    fn gap(&self, stream_id: u32, reason: &str);
    fn end(&self, stream_id: u32, reason: &str);
}

#[derive(Clone)]
pub struct BridgeHandle {
    control_tx: mpsc::UnboundedSender<BridgeCommand>,
    audio_tx: mpsc::Sender<AudioFrame>,
    state: Arc<RwLock<BridgeState>>,
}

#[derive(Default)]
struct BridgeState {
    connected: bool,
    streams: HashMap<u32, StreamOpen>,
}

impl BridgeHandle {
    pub fn spawn(url: String, secret: String) -> (Arc<Self>, broadcast::Receiver<ServerMessage>) {
        Self::spawn_with_shutdown(url, secret, tokio_util::sync::CancellationToken::new())
    }

    pub fn spawn_with_shutdown(
        url: String,
        secret: String,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> (Arc<Self>, broadcast::Receiver<ServerMessage>) {
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (audio_tx, audio_rx) = mpsc::channel(512);
        let (result_tx, result_rx) = broadcast::channel(256);
        let state = Arc::new(RwLock::new(BridgeState::default()));
        let handle = Arc::new(Self {
            control_tx,
            audio_tx,
            state: Arc::clone(&state),
        });
        tokio::spawn(async move {
            tokio::select! {
                _ = shutdown.cancelled() => {},
                _ = run_bridge(url, secret, control_rx, audio_rx, state.clone(), result_tx) => {},
            }
            state.write().expect("bridge state poisoned").connected = false;
        });
        (handle, result_rx)
    }

    pub fn is_connected(&self) -> bool {
        self.state.read().expect("bridge state poisoned").connected
    }

    fn control(&self, command: BridgeCommand) {
        let _ = self.control_tx.send(command);
    }
}

#[async_trait]
impl AudioBridge for BridgeHandle {
    fn open(&self, stream: StreamOpen) {
        let connected = {
            let mut state = self.state.write().expect("bridge state poisoned");
            state.streams.insert(stream.stream_id, stream.clone());
            state.connected
        };
        if connected {
            self.control(BridgeCommand::Open(stream));
        }
    }

    fn audio(&self, frame: AudioFrame) -> bool {
        self.is_connected() && self.audio_tx.try_send(frame).is_ok()
    }

    fn idle(&self, stream_id: u32) {
        if self.is_connected() {
            self.control(BridgeCommand::Idle(stream_id));
        }
    }

    fn gap(&self, stream_id: u32, reason: &str) {
        if self.is_connected() {
            self.control(BridgeCommand::Gap(stream_id, reason.to_owned()));
        }
    }

    fn end(&self, stream_id: u32, reason: &str) {
        let connected = {
            let mut state = self.state.write().expect("bridge state poisoned");
            state.streams.remove(&stream_id);
            state.connected
        };
        if connected {
            self.control(BridgeCommand::End(stream_id, reason.to_owned()));
        }
    }
}

async fn run_bridge(
    url: String,
    secret: String,
    mut controls: mpsc::UnboundedReceiver<BridgeCommand>,
    mut audio: mpsc::Receiver<AudioFrame>,
    state: Arc<RwLock<BridgeState>>,
    results: broadcast::Sender<ServerMessage>,
) {
    let mut retry = false;
    loop {
        if retry {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        retry = true;
        let connection = connect_async(&url).await;
        let (socket, _) = match connection {
            Ok(value) => value,
            Err(error) => {
                warn!(%error, "hayamimi bridge connection failed");
                continue;
            }
        };
        let (mut writer, mut reader) = socket.split();
        let epoch = Uuid::new_v4().to_string();
        let hello = serde_json::to_string(&Hello::new(&secret, &epoch)).expect("hello serializes");
        if writer.send(Message::Text(hello.into())).await.is_err() {
            continue;
        }
        let ready = tokio::time::timeout(Duration::from_secs(5), reader.next()).await;
        let Some(Ok(Message::Text(text))) = ready.ok().flatten() else {
            warn!("hayamimi did not complete protocol handshake");
            continue;
        };
        let Ok(message) = serde_json::from_str::<ServerMessage>(&text) else {
            warn!("hayamimi returned invalid handshake JSON");
            continue;
        };
        if message.kind != "ready" || message.protocol != Some(2) {
            warn!(kind = %message.kind, error = ?message.message,
                  "hayamimi rejected protocol handshake");
            continue;
        }
        // Anything queued belongs to a connection that is already gone. Stream
        // lifecycle is recovered from the registry; stale PCM must not be replayed.
        while controls.try_recv().is_ok() {}
        while audio.try_recv().is_ok() {}
        let reopen: Vec<_> = {
            let mut state = state.write().expect("bridge state poisoned");
            let values = state.streams.values().cloned().collect();
            state.connected = true;
            values
        };
        let mut reopen_failed = false;
        for stream in reopen {
            if send_json(&mut writer, &stream).await.is_err() {
                reopen_failed = true;
                break;
            }
        }
        if reopen_failed {
            state.write().expect("bridge state poisoned").connected = false;
            continue;
        }
        info!(%epoch, "connected to hayamimi ingest v2");

        loop {
            tokio::select! {
                biased;
                command = controls.recv() => {
                    let Some(command) = command else { return; };
                    if send_command(&mut writer, command).await.is_err() {
                        break;
                    }
                }
                frame = audio.recv() => {
                    let Some(frame) = frame else { return; };
                    // Revocation/stop wins over queued PCM, even when controls
                    // and audio were enqueued before this writer was scheduled.
                    if !state.read().expect("bridge state poisoned").streams.contains_key(&frame.stream_id) {
                        continue;
                    }
                    if send_audio(&mut writer, frame).await.is_err() {
                        break;
                    }
                }
                incoming = reader.next() => {
                    match incoming {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<ServerMessage>(&text) {
                                Ok(value) => { let _ = results.send(value); }
                                Err(error) => warn!(%error, "invalid result JSON from hayamimi"),
                            }
                        }
                        Some(Ok(Message::Ping(payload))) if writer.send(Message::Pong(payload.clone())).await.is_err() => break,
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    }
                }
            }
        }
        state.write().expect("bridge state poisoned").connected = false;
        warn!("hayamimi bridge disconnected; reconnecting");
    }
}

async fn send_command<S>(writer: &mut S, command: BridgeCommand) -> anyhow::Result<()>
where
    S: futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    match command {
        BridgeCommand::Open(value) => send_json(writer, &value).await?,
        BridgeCommand::Idle(stream_id) => {
            send_json(
                writer,
                &StreamControl {
                    kind: "stream_idle",
                    stream_id,
                    reason: None,
                },
            )
            .await?;
        }
        BridgeCommand::Gap(stream_id, reason) => {
            send_json(
                writer,
                &StreamControl {
                    kind: "gap",
                    stream_id,
                    reason: Some(&reason),
                },
            )
            .await?;
        }
        BridgeCommand::End(stream_id, reason) => {
            send_json(
                writer,
                &StreamControl {
                    kind: "stream_end",
                    stream_id,
                    reason: Some(&reason),
                },
            )
            .await?;
        }
    }
    Ok(())
}

async fn send_audio<S>(writer: &mut S, frame: AudioFrame) -> anyhow::Result<()>
where
    S: futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    writer.send(Message::Binary(frame.encode()?.into())).await?;
    Ok(())
}

async fn send_json<S, T>(writer: &mut S, value: &T) -> anyhow::Result<()>
where
    S: futures::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    T: serde::Serialize,
{
    writer
        .send(Message::Text(serde_json::to_string(value)?.into()))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use serde_json::Map;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    fn test_handle(
        audio_capacity: usize,
    ) -> (
        BridgeHandle,
        mpsc::UnboundedReceiver<BridgeCommand>,
        mpsc::Receiver<AudioFrame>,
    ) {
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (audio_tx, audio_rx) = mpsc::channel(audio_capacity);
        (
            BridgeHandle {
                control_tx,
                audio_tx,
                state: Arc::new(RwLock::new(BridgeState::default())),
            },
            control_rx,
            audio_rx,
        )
    }

    fn stream(id: u32) -> StreamOpen {
        StreamOpen::new(id, "user", "Alice", 1, Map::new())
    }

    fn frame(sequence: u32) -> AudioFrame {
        AudioFrame {
            stream_id: 1,
            sequence,
            captured_at_ms: 1,
            pcm: vec![0, 0],
        }
    }

    #[test]
    fn disconnected_lifecycle_is_recovered_from_registry() {
        let (handle, mut controls, _audio) = test_handle(1);
        handle.open(stream(1));
        assert!(controls.try_recv().is_err());
        assert!(handle.state.read().unwrap().streams.contains_key(&1));

        handle.state.write().unwrap().connected = true;
        handle.open(stream(2));
        assert!(
            matches!(controls.try_recv(), Ok(BridgeCommand::Open(value)) if value.stream_id == 2)
        );
        handle.end(2, "done");
        assert!(
            matches!(controls.try_recv(), Ok(BridgeCommand::End(2, reason)) if reason == "done")
        );
    }

    #[test]
    fn audio_queue_is_bounded_without_dropping_control_messages() {
        let (handle, mut controls, _audio) = test_handle(1);
        handle.state.write().unwrap().connected = true;
        assert!(handle.audio(frame(1)));
        assert!(!handle.audio(frame(2)));
        handle.gap(1, "overflow");
        assert!(
            matches!(controls.try_recv(), Ok(BridgeCommand::Gap(1, reason)) if reason == "overflow")
        );
    }

    #[tokio::test]
    async fn websocket_handshake_sends_open_before_binary_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(tcp).await.unwrap();
            let hello = socket.next().await.unwrap().unwrap().into_text().unwrap();
            let hello: serde_json::Value = serde_json::from_str(&hello).unwrap();
            assert_eq!(hello["protocol"], 2);
            assert_eq!(hello["auth"], "secret");
            socket
                .send(Message::Text(
                    r#"{"type":"ready","protocol":2,"sr":16000,"max_streams":32}"#.into(),
                ))
                .await
                .unwrap();
            let opened = socket.next().await.unwrap().unwrap().into_text().unwrap();
            let opened: serde_json::Value = serde_json::from_str(&opened).unwrap();
            let binary = socket.next().await.unwrap().unwrap().into_data();
            (opened, AudioFrame::decode(&binary).unwrap())
        });

        let (handle, _results) =
            BridgeHandle::spawn(format!("ws://{address}/ingest/v2"), "secret".to_owned());
        for _ in 0..100 {
            if handle.is_connected() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(handle.is_connected());
        handle.open(stream(17));
        assert!(handle.audio(AudioFrame {
            stream_id: 17,
            sequence: 42,
            captured_at_ms: 123,
            pcm: vec![0, 0, 1, 0],
        }));

        let (opened, audio) = server.await.unwrap();
        assert_eq!(opened["type"], "stream_open");
        assert_eq!(opened["stream_id"], 17);
        assert_eq!(audio.stream_id, 17);
        assert_eq!(audio.sequence, 42);
    }
}
