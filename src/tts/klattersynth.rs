//! HTTP client for klattersynth-rs, using a base frequency of 230 Hz.
//! Other synthesis settings use the server defaults.
//! Override the endpoint with `NCB_KLATTERSYNTH_API_URL` or `klattersynth_api_url`
//! in config.toml. Like the other settings, config.toml takes precedence over env.

use std::fmt;

use serde::Serialize;

use crate::errors::{NCBError, Result};

use super::http;

pub const DEFAULT_API_URL: &str = "http://172.31.1.6:3000";

pub struct KlattersynthTTS {
    client: reqwest::Client,
    synthesis_url: String,
}

impl fmt::Debug for KlattersynthTTS {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KlattersynthTTS").finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct SynthesisRequest<'a> {
    text: &'a str,
    config: SynthesisConfig,
}

#[derive(Serialize)]
struct SynthesisConfig {
    voice_base_frequency: u32,
}

impl KlattersynthTTS {
    pub fn new(api_url: Option<String>) -> Result<Self> {
        let api_url = api_url.unwrap_or_else(|| DEFAULT_API_URL.to_string());
        let synthesis_url = format!("{}/synthesize", api_url.trim_end_matches('/'));
        let url = reqwest::Url::parse(&synthesis_url)
            .map_err(|e| NCBError::config(format!("Invalid Klattersynth API URL: {e}")))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(NCBError::config(
                "Klattersynth API URL must use HTTP or HTTPS",
            ));
        }

        Ok(Self {
            client: http::client()?,
            synthesis_url,
        })
    }

    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        let response = http::check_status(
            self.client
                .post(&self.synthesis_url)
                .json(&SynthesisRequest {
                    text,
                    config: SynthesisConfig {
                        voice_base_frequency: 230,
                    },
                })
                .send()
                .await?,
            "Klattersynth",
        )?;

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if content_type.split(';').next().map(str::trim) != Some("audio/wav") {
            return Err(NCBError::ApiResponse {
                service: "Klattersynth",
                reason: "expected audio/wav Content-Type",
            });
        }

        Ok(response.bytes().await?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    async fn mock_server(
        status: &'static str,
        content_type: &'static str,
        body: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<serde_json::Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let json = loop {
                let mut buf = [0; 1024];
                let read = stream.read(&mut buf).await.unwrap();
                assert!(read > 0, "client closed before sending request body");
                request.extend_from_slice(&buf[..read]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    assert!(headers.starts_with("post /synthesize http/1.1\r\n"));
                    assert!(headers.contains("content-type: application/json"));
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break serde_json::from_slice(&request[end + 4..end + 4 + length]).unwrap();
                    }
                }
            };
            let headers = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(headers.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
            json
        });
        (url, task)
    }

    fn wav() -> Vec<u8> {
        // A short 11025 Hz, mono, signed 16-bit PCM waveform, like the API.
        let data_len = 220u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&11025u32.to_le_bytes());
        wav.extend_from_slice(&22050u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.resize(44 + data_len as usize, 0);
        wav
    }

    #[tokio::test]
    async fn sends_plain_text_json_and_decodes_wav_with_songbird() {
        let expected = wav();
        let (url, server) = mock_server("200 OK", "audio/wav", expected.clone()).await;
        let client = KlattersynthTTS::new(Some(url)).unwrap();
        let text = "Hello \"world\"!\nこんにちは";
        let audio = client.synthesize(text).await.unwrap();
        assert_eq!(
            server.await.unwrap(),
            serde_json::json!({
                "text": text,
                "config": {"voice_base_frequency": 230},
            })
        );
        assert_eq!(audio, expected);

        let input: songbird::input::Input = audio.into();
        let mut input = input
            .make_playable_async(
                songbird::input::codecs::get_codec_registry(),
                songbird::input::codecs::get_probe(),
            )
            .await
            .unwrap();
        let parsed = input.parsed_mut().unwrap();
        let packet = parsed.format.next_packet().unwrap();
        let decoded = parsed.decoder.decode(&packet).unwrap();
        assert_eq!(decoded.spec().rate, 11025);
        assert_eq!(decoded.spec().channels.count(), 1);
        assert!(decoded.frames() > 0);
    }

    #[tokio::test]
    async fn rejects_http_errors_and_non_audio_success_responses() {
        for (status, content_type, expected_error) in [
            ("422 Unprocessable Entity", "application/json", "422"),
            ("503 Service Unavailable", "application/json", "503"),
            ("200 OK", "application/json", "Content-Type"),
        ] {
            let (url, server) =
                mock_server(status, content_type, b"{\"error\":\"test\"}".to_vec()).await;
            let client = KlattersynthTTS::new(Some(url)).unwrap();
            let error = client.synthesize("Hello").await.unwrap_err();
            assert!(error.to_string().contains(expected_error), "{error}");
            server.await.unwrap();
        }
    }

    #[test]
    fn endpoint_defaults_and_invalid_configuration() {
        let client = KlattersynthTTS::new(None).unwrap();
        assert_eq!(client.synthesis_url, "http://172.31.1.6:3000/synthesize");
        assert!(KlattersynthTTS::new(Some("file:///tmp/tts".into())).is_err());
        assert!(KlattersynthTTS::new(Some("invalid".into())).is_err());
    }

    #[tokio::test]
    #[ignore = "requires the live Klattersynth server; set NCB_KLATTERSYNTH_API_URL to override"]
    async fn live_server_audio_is_playable() {
        let client = KlattersynthTTS::new(std::env::var("NCB_KLATTERSYNTH_API_URL").ok()).unwrap();
        let audio = client.synthesize("Hello, world!").await.unwrap();
        let input: songbird::input::Input = audio.into();
        let mut input = input
            .make_playable_async(
                songbird::input::codecs::get_codec_registry(),
                songbird::input::codecs::get_probe(),
            )
            .await
            .unwrap();
        let parsed = input.parsed_mut().unwrap();
        let packet = parsed.format.next_packet().unwrap();
        let decoded = parsed.decoder.decode(&packet).unwrap();
        assert_eq!(decoded.spec().rate, 11025);
        assert!(decoded.frames() > 0);
    }
}
