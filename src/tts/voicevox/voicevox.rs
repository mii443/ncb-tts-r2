use super::structs::speaker::Speaker;

const BASE_API_URL: &str = "https://deprecatedapis.tts.quest/v2/";

#[derive(Clone, Debug)]
pub struct VOICEVOX {
    pub key: String,
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

    pub fn new(key: String) -> Self {
        Self { key }
    }

    #[tracing::instrument]
    async fn get_speaker_list(&self) -> Vec<Speaker> {
        let client = reqwest::Client::new();
        match client
            .post(BASE_API_URL.to_string() + "voicevox/speakers/")
            .query(&[("key", self.key.clone())])
            .send()
            .await
        {
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
                ("key", self.key.clone()),
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
}
