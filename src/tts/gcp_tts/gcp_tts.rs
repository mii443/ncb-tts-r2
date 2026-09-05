use crate::{
    errors::{constants::TTS_TIMEOUT_SECS, NCBError, Result},
    tts::{
        gcp_tts::structs::{
            synthesize_request::SynthesizeRequest, synthesize_response::SynthesizeResponse,
        },
        http,
    },
};
use base64::{engine::general_purpose, Engine as _};
use gcp_auth::Token;
use std::{fmt, sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct GCPTTS {
    token: Arc<RwLock<Token>>,
    credentials_path: String,
    client: reqwest::Client,
    endpoint: String,
}

impl fmt::Debug for GCPTTS {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GCPTTS").finish_non_exhaustive()
    }
}

impl GCPTTS {
    #[tracing::instrument(skip_all)]
    pub async fn update_token(&self) -> Result<()> {
        if !self.token.read().await.has_expired() {
            return Ok(());
        }
        tokio::time::timeout(Duration::from_secs(TTS_TIMEOUT_SECS), async {
            let mut token = self.token.write().await;
            if token.has_expired() {
                let authenticator =
                    gcp_auth::from_credentials_file(self.credentials_path.clone()).await?;
                *token = authenticator
                    .get_token(&["https://www.googleapis.com/auth/cloud-platform"])
                    .await?;
            }
            Ok::<_, NCBError>(())
        })
        .await
        .map_err(|_| NCBError::Timeout {
            operation: "GCP authentication",
        })?
    }

    #[tracing::instrument(skip_all)]
    pub async fn new(credentials_path: String) -> Result<Self> {
        let client = http::client()?;
        let token = tokio::time::timeout(Duration::from_secs(TTS_TIMEOUT_SECS), async {
            let authenticator = gcp_auth::from_credentials_file(credentials_path.clone()).await?;
            authenticator
                .get_token(&["https://www.googleapis.com/auth/cloud-platform"])
                .await
        })
        .await
        .map_err(|_| NCBError::Timeout {
            operation: "GCP authentication",
        })??;
        Ok(Self {
            token: Arc::new(RwLock::new(token)),
            credentials_path,
            client,
            endpoint: "https://texttospeech.googleapis.com/v1/text:synthesize".into(),
        })
    }

    /// Synthesize one request. Retry policy belongs to the TTS service.
    #[tracing::instrument(skip_all)]
    pub async fn synthesize(&self, request: SynthesizeRequest) -> Result<Vec<u8>> {
        self.update_token().await?;
        let token = self.token.read().await.as_str().to_owned();
        let response = http::check_status(
            self.client
                .post(&self.endpoint)
                .bearer_auth(token)
                .json(&request)
                .send()
                .await?,
            "GCP",
        )?;
        let response: SynthesizeResponse =
            http::json(response, "GCP", "expected audioContent").await?;
        general_purpose::STANDARD
            .decode(response.audioContent)
            .map_err(|_| NCBError::ApiResponse {
                service: "GCP",
                reason: "invalid base64 audio",
            })
    }

    #[cfg(test)]
    pub(crate) fn for_test(endpoint: String) -> Self {
        let token = serde_json::from_str::<Token>(
            r#"{"access_token":"test-secret-token","expires_in":3600}"#,
        )
        .unwrap();
        Self {
            token: Arc::new(RwLock::new(token)),
            credentials_path: "unused".into(),
            client: http::client().unwrap(),
            endpoint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_token_refresh_returns_error_and_preserves_existing_token() {
        let directory = tempfile::tempdir().unwrap();
        let mut client = GCPTTS::for_test("http://127.0.0.1:1".into());
        client.credentials_path = directory
            .path()
            .join("missing.json")
            .to_string_lossy()
            .into_owned();
        *client.token.write().await = serde_json::from_str::<Token>(
            r#"{"access_token":"expired-secret-token","expires_in":0}"#,
        )
        .unwrap();
        assert!(client.update_token().await.is_err());
        assert_eq!(client.token.read().await.as_str(), "expired-secret-token");
    }
}
