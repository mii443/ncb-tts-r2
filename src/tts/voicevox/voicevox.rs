const API_URL: &str = "https://api.su-shiki.com/v2/voicevox/audio";

#[derive(Clone)]
pub struct VOICEVOX {
    pub key: String
}

impl VOICEVOX {
    pub fn new(key: String) -> Self {
        Self {
            key
        }
    }

    pub async fn synthesize(&self, text: String, speaker: i64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        match client.post(API_URL).query(&[("speaker", speaker.to_string()), ("text", text), ("key", self.key.clone())]).send().await {
            Ok(response) => {
                let body = response.bytes().await?;
                Ok(body.to_vec())
            }
            Err(err) => {
                Err(Box::new(err))
            }
        }
    }
}