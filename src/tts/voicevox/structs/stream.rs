use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TTSResponse {
    pub success: bool,
    pub is_api_key_valid: bool,
    pub speaker_name: String,
    pub audio_status_url: String,
    pub wav_download_url: String,
    pub mp3_download_url: String,
    pub mp3_streaming_url: String,
}
