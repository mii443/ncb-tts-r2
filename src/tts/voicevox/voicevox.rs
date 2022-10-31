const API_URL: &str = "https://api.su-shiki.com/v2/voicevox/audio";

#[derive(Clone)]
pub struct VOICEVOX {
    pub key: String,
}

impl VOICEVOX {
    pub fn get_speakers() -> Vec<(String, i64)> {
        vec![
            ("四国めたん - ノーマル".to_string(), 2),
            ("四国めたん - あまあま".to_string(), 0),
            ("四国めたん - ツンツン".to_string(), 6),
            ("四国めたん - セクシー".to_string(), 4),
            ("ずんだもん - ノーマル".to_string(), 3),
            ("ずんだもん - あまあま".to_string(), 1),
            ("ずんだもん - ツンツン".to_string(), 7),
            ("ずんだもん - セクシー".to_string(), 5),
            ("春日部つむぎ - ノーマル".to_string(), 8),
            ("雨晴はう - ノーマル".to_string(), 10),
            ("波音リツ - ノーマル".to_string(), 9),
            ("玄野武宏 - ノーマル".to_string(), 11),
            ("白上虎太郎 - ノーマル".to_string(), 12),
            ("青山龍星 - ノーマル".to_string(), 13),
            ("冥鳴ひまり - ノーマル".to_string(), 14),
            ("九州そら - ノーマル".to_string(), 16),
            ("九州そら - あまあま".to_string(), 15),
            ("九州そら - ツンツン".to_string(), 18),
            ("九州そら - セクシー".to_string(), 17),
            ("九州そら - ささやき".to_string(), 19),
            ("モチノ・キョウコ - ノーマル".to_string(), 20),
        ]
    }

    pub fn new(key: String) -> Self {
        Self { key }
    }

    pub async fn synthesize(
        &self,
        text: String,
        speaker: i64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        match client
            .post(API_URL)
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
