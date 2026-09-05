use std::fmt;

use super::structs::speaker::Speaker;
use crate::{
    errors::{NCBError, Result},
    stream_input::Mp3Request,
    tts::http,
};

const BASE_API_URL: &str = "https://deprecatedapis.tts.quest/v2";
const STREAM_API_URL: &str = "https://api.tts.quest/v3/voicevox/synthesis";

#[derive(Clone)]
pub struct VOICEVOX {
    key: Option<String>,
    pub original_api_url: Option<String>,
    client: reqwest::Client,
    stream_api_url: String,
}

impl fmt::Debug for VOICEVOX {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VOICEVOX")
            .field("has_api_key", &self.key.is_some())
            .field("has_original_api", &self.original_api_url.is_some())
            .finish()
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamResponse {
    success: bool,
    is_api_key_valid: Option<bool>,
    mp3_streaming_url: Option<String>,
}

impl VOICEVOX {
    pub fn new(key: Option<String>, original_api_url: Option<String>) -> Result<Self> {
        Ok(Self {
            key,
            original_api_url,
            client: http::client()?,
            stream_api_url: STREAM_API_URL.into(),
        })
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_styles(&self) -> Result<Vec<(String, i64)>> {
        Ok(self
            .get_speaker_list()
            .await?
            .into_iter()
            .flat_map(|speaker| {
                speaker
                    .styles
                    .into_iter()
                    .map(move |style| (format!("{} - {}", speaker.name, style.name), style.id))
            })
            .collect())
    }

    #[tracing::instrument(skip_all)]
    pub async fn get_speakers(&self) -> Result<Vec<String>> {
        Ok(self
            .get_speaker_list()
            .await?
            .into_iter()
            .map(|speaker| speaker.name)
            .collect())
    }

    #[tracing::instrument(skip_all)]
    async fn get_speaker_list(&self) -> Result<Vec<Speaker>> {
        let request = if let Some(url) = &self.original_api_url {
            self.client
                .get(format!("{}/speakers", url.trim_end_matches('/')))
        } else if let Some(key) = &self.key {
            self.client
                .get(format!("{BASE_API_URL}/voicevox/speakers/"))
                .query(&[("key", key)])
        } else {
            return Err(NCBError::voicevox(
                "No API key or original API URL provided",
            ));
        };
        http::json(
            http::check_status(request.send().await?, "VOICEVOX")?,
            "VOICEVOX",
            "invalid speakers",
        )
        .await
    }

    #[tracing::instrument(skip_all)]
    pub async fn synthesize(&self, text: String, speaker: i64) -> Result<Vec<u8>> {
        let key = self
            .key
            .as_ref()
            .ok_or_else(|| NCBError::voicevox("API key required for synthesis"))?;
        let response = http::check_status(
            self.client
                .post(format!("{BASE_API_URL}/voicevox/audio/"))
                .query(&[
                    ("speaker", speaker.to_string()),
                    ("text", text),
                    ("key", key.clone()),
                ])
                .send()
                .await?,
            "VOICEVOX",
        )?;
        Ok(response.bytes().await?.to_vec())
    }

    #[tracing::instrument(skip_all)]
    pub async fn synthesize_original(&self, text: String, speaker: i64) -> Result<Vec<u8>> {
        let url = self
            .original_api_url
            .as_ref()
            .ok_or_else(|| NCBError::voicevox("Original API URL required for synthesis"))?
            .trim_end_matches('/');
        let response = http::check_status(
            self.client
                .post(format!("{url}/audio_query"))
                .query(&[("text", text), ("speaker", speaker.to_string())])
                .send()
                .await?,
            "VOICEVOX",
        )?;
        let query: serde_json::Value =
            http::json(response, "VOICEVOX", "invalid audio query").await?;
        let audio = http::check_status(
            self.client
                .post(format!("{url}/synthesis"))
                .query(&[
                    ("speaker", speaker.to_string()),
                    ("enable_interrogative_upspeak", "true".into()),
                ])
                .json(&query)
                .send()
                .await?,
            "VOICEVOX",
        )?
        .bytes()
        .await?;
        Ok(audio.to_vec())
    }

    #[tracing::instrument(skip_all)]
    pub async fn synthesize_stream(&self, text: String, speaker: i64) -> Result<Mp3Request> {
        let key = self
            .key
            .as_ref()
            .ok_or_else(|| NCBError::voicevox("API key required for stream synthesis"))?;
        let response = http::check_status(
            self.client
                .post(&self.stream_api_url)
                .query(&[
                    ("speaker", speaker.to_string()),
                    ("text", text),
                    ("key", key.clone()),
                ])
                .send()
                .await?,
            "VOICEVOX",
        )?;
        let response: StreamResponse =
            http::json(response, "VOICEVOX", "invalid stream response").await?;
        if !response.success || response.is_api_key_valid == Some(false) {
            return Err(NCBError::ApiResponse {
                service: "VOICEVOX",
                reason: "stream synthesis rejected",
            });
        }
        let url = response
            .mp3_streaming_url
            .filter(|url| !url.is_empty())
            .ok_or(NCBError::ApiResponse {
                service: "VOICEVOX",
                reason: "missing streaming URL",
            })?;
        let parsed = reqwest::Url::parse(&url).map_err(|_| NCBError::ApiResponse {
            service: "VOICEVOX",
            reason: "invalid streaming URL",
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(NCBError::ApiResponse {
                service: "VOICEVOX",
                reason: "invalid streaming URL scheme",
            });
        }
        Ok(Mp3Request::new(self.client.clone(), url))
    }

    #[cfg(test)]
    pub(crate) fn for_test(stream_api_url: String, original_api_url: Option<String>) -> Self {
        Self {
            key: Some("test-secret-key".into()),
            original_api_url,
            client: http::client().unwrap(),
            stream_api_url,
        }
    }
}
