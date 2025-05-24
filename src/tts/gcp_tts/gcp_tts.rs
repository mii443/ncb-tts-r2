use crate::tts::gcp_tts::structs::{
    synthesize_request::SynthesizeRequest, synthesize_response::SynthesizeResponse,
};
use gcp_auth::Token;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct GCPTTS {
    pub token: Arc<RwLock<Token>>,
    pub credentials_path: String,
}

impl GCPTTS {
    #[tracing::instrument]
    pub async fn update_token(&self) -> Result<(), gcp_auth::Error> {
        let mut token = self.token.write().await;
        if token.has_expired() {
            let authenticator =
                gcp_auth::from_credentials_file(self.credentials_path.clone()).await?;
            let new_token = authenticator
                .get_token(&["https://www.googleapis.com/auth/cloud-platform"])
                .await?;
            *token = new_token;
        }

        Ok(())
    }

    #[tracing::instrument]
    pub async fn new(credentials_path: String) -> Result<Self, gcp_auth::Error> {
        let authenticator = gcp_auth::from_credentials_file(credentials_path.clone()).await?;
        let token = authenticator
            .get_token(&["https://www.googleapis.com/auth/cloud-platform"])
            .await?;

        Ok(Self {
            token: Arc::new(RwLock::new(token)),
            credentials_path,
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
    #[tracing::instrument]
    pub async fn synthesize(
        &self,
        request: SynthesizeRequest,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.update_token().await.unwrap();
        let client = reqwest::Client::new();

        let token_string = {
            let token = self.token.read().await;
            token.as_str().to_string()
        };

        match client
            .post("https://texttospeech.googleapis.com/v1/text:synthesize")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", token_string),
            )
            .body(serde_json::to_string(&request).unwrap())
            .send()
            .await
        {
            Ok(ok) => {
                let response: SynthesizeResponse =
                    serde_json::from_str(&ok.text().await.expect("")).unwrap();
                Ok(base64::decode(response.audioContent).unwrap()[..].to_vec())
            }
            Err(err) => Err(Box::new(err)),
        }
    }
}
