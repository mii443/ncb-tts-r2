use gcp_auth::Token;
use crate::tts::gcp_tts::structs::{
    synthesize_request::SynthesizeRequest,
    synthesize_response::SynthesizeResponse,
};

#[derive(Clone)]
pub struct TTS {
    pub token: Token,
    pub credentials_path: String
}

impl TTS {

    pub async fn update_token(&mut self) -> Result<(), gcp_auth::Error> {
        if self.token.has_expired() {
            let authenticator = gcp_auth::from_credentials_file(self.credentials_path.clone()).await?;
            let token = authenticator.get_token(&["https://www.googleapis.com/auth/cloud-platform"]).await?;
            self.token = token;
        }

        Ok(())
    }

    pub async fn new(credentials_path: String) -> Result<TTS, gcp_auth::Error> {
        let authenticator = gcp_auth::from_credentials_file(credentials_path.clone()).await?;
        let token = authenticator.get_token(&["https://www.googleapis.com/auth/cloud-platform"]).await?;

        Ok(TTS {
            token,
            credentials_path
        })
    }

    /// Synthesize text to speech and return the audio data.
    ///
    /// Example:
    /// ```rust
    /// let audio = storage.synthesize(SynthesizeRequest {
    ///    input: SynthesisInput {
    ///        text: None,
    ///        ssml: Some(String::from("<speak>test</speak>"))
    ///    },
    ///    voice: VoiceSelectionParams {
    ///        languageCode: String::from("ja-JP"),
    ///        name: String::from("ja-JP-Wavenet-B"),
    ///        ssmlGender: String::from("neutral")
    ///    },
    ///    audioConfig: AudioConfig {
    ///        audioEncoding: String::from("mp3"),
    ///        speakingRate: 1.2f32,
    ///        pitch: 1.0f32
    ///    }
    /// }).await.unwrap();
    /// ```
    pub async fn synthesize(&mut self, request: SynthesizeRequest) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.update_token().await.unwrap();
        let client = reqwest::Client::new();
        match client.post("https://texttospeech.googleapis.com/v1/text:synthesize")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", self.token.as_str()))
            .body(serde_json::to_string(&request).unwrap())
            .send().await {
                Ok(ok) => {
                    let response: SynthesizeResponse = serde_json::from_str(&ok.text().await.expect("")).unwrap();
                    Ok(base64::decode(response.audioContent).unwrap()[..].to_vec())
                },
                Err(err) => Err(Box::new(err))
        }
    }
}
