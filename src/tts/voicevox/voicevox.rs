use crate::{errors::NCBError, stream_input::Mp3Request};

use super::structs::{speaker::Speaker, stream::TTSResponse};

const BASE_API_URL: &str = "https://deprecatedapis.tts.quest/v2/";
const STREAM_API_URL: &str = "https://api.tts.quest/v3/voicevox/synthesis";

#[derive(Clone, Debug)]
pub struct VOICEVOX {
    pub key: Option<String>,
    pub original_api_url: Option<String>,
}

impl VOICEVOX {
    #[tracing::instrument]
    pub async fn get_styles(&self) -> Result<Vec<(String, i64)>, NCBError> {
        let speakers = self.get_speaker_list().await?;
        let mut speaker_list = Vec::new();
        for speaker in speakers {
            for style in speaker.styles {
                speaker_list.push((format!("{} - {}", speaker.name, style.name), style.id))
            }
        }

        Ok(speaker_list)
    }

    #[tracing::instrument]
    pub async fn get_speakers(&self) -> Result<Vec<String>, NCBError> {
        let speakers = self.get_speaker_list().await?;
        let mut speaker_list = Vec::new();
        for speaker in speakers {
            speaker_list.push(speaker.name)
        }

        Ok(speaker_list)
    }

    pub fn new(key: Option<String>, original_api_url: Option<String>) -> Self {
        Self {
            key,
            original_api_url,
        }
    }

    #[tracing::instrument]
    async fn get_speaker_list(&self) -> Result<Vec<Speaker>, NCBError> {
        let client = reqwest::Client::new();
        let request = if let Some(key) = &self.key {
            client
                .get(format!("{}{}", BASE_API_URL, "voicevox/speakers/"))
                .query(&[("key", key)])
        } else if let Some(original_api_url) = &self.original_api_url {
            client.get(format!("{}/speakers", original_api_url))
        } else {
            return Err(NCBError::voicevox("No API key or original API URL provided"));
        };

        let response = request.send().await
            .map_err(|e| NCBError::voicevox(format!("Failed to fetch speakers: {}", e)))?;

        if !response.status().is_success() {
            return Err(NCBError::voicevox(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        response.json().await
            .map_err(|e| NCBError::voicevox(format!("Failed to parse speaker list: {}", e)))
    }

    #[tracing::instrument]
    pub async fn synthesize(
        &self,
        text: String,
        speaker: i64,
    ) -> Result<Vec<u8>, NCBError> {
        let key = self.key.as_ref()
            .ok_or_else(|| NCBError::voicevox("API key required for synthesis"))?;
        
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}{}", BASE_API_URL, "voicevox/audio/"))
            .query(&[
                ("speaker", speaker.to_string()),
                ("text", text),
                ("key", key.clone()),
            ])
            .send()
            .await
            .map_err(|e| NCBError::voicevox(format!("Synthesis request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(NCBError::voicevox(format!(
                "Synthesis failed with status: {}",
                response.status()
            )));
        }

        let body = response.bytes().await
            .map_err(|e| NCBError::voicevox(format!("Failed to read response body: {}", e)))?;
        
        Ok(body.to_vec())
    }

    #[tracing::instrument]
    pub async fn synthesize_original(
        &self,
        text: String,
        speaker: i64,
    ) -> Result<Vec<u8>, NCBError> {
        let api_url = self.original_api_url.as_ref()
            .ok_or_else(|| NCBError::voicevox("Original API URL required for synthesis"))?;
        
        let client = voicevox_client::Client::new(api_url.clone(), None);
        let audio_query = client
            .create_audio_query(&text, speaker as i32, None)
            .await
            .map_err(|e| NCBError::voicevox(format!("Failed to create audio query: {}", e)))?;
        
        tracing::debug!(audio_query = ?audio_query.audio_query, "Generated audio query");
        
        let audio = audio_query.synthesis(speaker as i32, true).await
            .map_err(|e| NCBError::voicevox(format!("Audio synthesis failed: {}", e)))?;
        
        Ok(audio.into())
    }

    #[tracing::instrument]
    pub async fn synthesize_stream(
        &self,
        text: String,
        speaker: i64,
    ) -> Result<Mp3Request, NCBError> {
        let key = self.key.as_ref()
            .ok_or_else(|| NCBError::voicevox("API key required for stream synthesis"))?;
        
        let client = reqwest::Client::new();
        let response = client
            .post(STREAM_API_URL)
            .query(&[
                ("speaker", speaker.to_string()),
                ("text", text),
                ("key", key.clone()),
            ])
            .send()
            .await
            .map_err(|e| NCBError::voicevox(format!("Stream synthesis request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(NCBError::voicevox(format!(
                "Stream synthesis failed with status: {}",
                response.status()
            )));
        }

        let body = response.text().await
            .map_err(|e| NCBError::voicevox(format!("Failed to read response text: {}", e)))?;
        
        let tts_response: TTSResponse = serde_json::from_str(&body)
            .map_err(|e| NCBError::voicevox(format!("Failed to parse TTS response: {}", e)))?;

        Ok(Mp3Request::new(reqwest::Client::new(), tts_response.mp3_streaming_url))
    }
}
