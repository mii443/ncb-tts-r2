use crate::stream_input::Mp3Request;

use super::structs::{speaker::Speaker, stream::TTSResponse};

const BASE_API_URL: &str = "https://deprecatedapis.tts.quest/v2/";

#[derive(Clone, Debug)]
pub struct VOICEVOX {
    pub key: Option<String>,
    pub original_api_url: Option<String>,
}

impl VOICEVOX {
    #[tracing::instrument]
    pub async fn get_styles(&self) -> Vec<(String, i64)> {
        let speakers = self.get_speaker_list().await;
        let mut speaker_list = vec![];
        for speaker in speakers {
            for style in speaker.styles {
                speaker_list.push((format!("{} - {}", speaker.name, style.name), style.id))
            }
        }

        speaker_list
    }

    #[tracing::instrument]
    pub async fn get_speakers(&self) -> Vec<String> {
        let speakers = self.get_speaker_list().await;
        let mut speaker_list = vec![];
        for speaker in speakers {
            speaker_list.push(speaker.name)
        }

        speaker_list
    }

    pub fn new(key: Option<String>, original_api_url: Option<String>) -> Self {
        Self {
            key,
            original_api_url,
        }
    }

    #[tracing::instrument]
    async fn get_speaker_list(&self) -> Vec<Speaker> {
        let client = reqwest::Client::new();
        let client = if let Some(key) = &self.key {
            client
                .get(BASE_API_URL.to_string() + "voicevox/speakers/")
                .query(&[("key", key)])
        } else if let Some(original_api_url) = &self.original_api_url {
            client.get(original_api_url.to_string() + "/speakers")
        } else {
            panic!("No API key or original API URL provided.")
        };

        match client.send().await {
            Ok(response) => response.json().await.unwrap(),
            Err(err) => {
                panic!("Cannot get speaker list. {err:?}")
            }
        }
    }

    #[tracing::instrument]
    pub async fn synthesize(
        &self,
        text: String,
        speaker: i64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        match client
            .post(BASE_API_URL.to_string() + "voicevox/audio/")
            .query(&[
                ("speaker", speaker.to_string()),
                ("text", text),
                ("key", self.key.clone().unwrap()),
            ])
            .send()
            .await
        {
            Ok(response) => {
                let body = response.bytes().await?;
                Ok(body.to_vec())
            }
            Err(err) => Err(Box::new(err)),
        }
    }

    #[tracing::instrument]
    pub async fn synthesize_original(
        &self,
        text: String,
        speaker: i64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client =
            voicevox_client::Client::new(self.original_api_url.as_ref().unwrap().clone(), None);
        let audio_query = client
            .create_audio_query(&text, speaker as i32, None)
            .await?;
        println!("{:?}", audio_query.audio_query);
        let audio = audio_query.synthesis(speaker as i32, true).await?;
        Ok(audio.into())
    }

    #[tracing::instrument]
    pub async fn synthesize_stream(
        &self,
        text: String,
        speaker: i64,
    ) -> Result<Mp3Request, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        match client
            .post("https://api.tts.quest/v3/voicevox/synthesis")
            .query(&[
                ("speaker", speaker.to_string()),
                ("text", text),
                ("key", self.key.clone().unwrap()),
            ])
            .send()
            .await
        {
            Ok(response) => {
                let body = response.text().await.unwrap();
                let response: TTSResponse = serde_json::from_str(&body).unwrap();

                Ok(Mp3Request::new(reqwest::Client::new(), response.mp3_streaming_url).into())
            }
            Err(err) => Err(Box::new(err)),
        }
    }
}
