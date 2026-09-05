use std::{future::Future, time::Duration};

use crate::errors::{constants::*, NCBError, Result};

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(TTS_TIMEOUT_SECS))
        .build()?)
}

pub fn check_status(
    response: reqwest::Response,
    service: &'static str,
) -> Result<reqwest::Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        // Do not include response bodies or URLs: either may echo credentials or text.
        Err(NCBError::ApiStatus {
            service,
            status: response.status().as_u16(),
        })
    }
}

pub async fn json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    service: &'static str,
    reason: &'static str,
) -> Result<T> {
    // Preserve retryable transport failures; never include the JSON body in errors.
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|_| NCBError::ApiResponse { service, reason })
}

pub async fn retry_synthesis<T, F, Fut>(mut operation: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    tokio::time::timeout(Duration::from_secs(TTS_TIMEOUT_SECS), async {
        let mut delay = Duration::from_millis(DEFAULT_RETRY_DELAY_MS);
        for attempt in 1..=DEFAULT_MAX_RETRY_ATTEMPTS {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) if error.is_retryable() && attempt < DEFAULT_MAX_RETRY_ATTEMPTS => {
                    tracing::warn!(attempt, error = %error, "Retrying TTS request");
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_millis(MAX_RETRY_DELAY_MS));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("retry limit is positive")
    })
    .await
    .map_err(|_| NCBError::Timeout {
        operation: "TTS synthesis",
    })?
}
