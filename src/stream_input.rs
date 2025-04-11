use async_trait::async_trait;
use futures::TryStreamExt;
use reqwest::{header::HeaderMap, Client};
use symphonia_core::{io::MediaSource, probe::Hint};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use songbird::input::{
    AsyncAdapterStream, AsyncReadOnlySource, AudioStream, AudioStreamError, Compose, Input,
};

#[derive(Debug, Clone)]
pub struct Mp3Request {
    client: Client,
    request: String,
    headers: HeaderMap,
}

impl Mp3Request {
    #[must_use]
    pub fn new(client: Client, request: String) -> Self {
        Self::new_with_headers(client, request, HeaderMap::default())
    }

    #[must_use]
    pub fn new_with_headers(client: Client, request: String, headers: HeaderMap) -> Self {
        Mp3Request {
            client,
            request,
            headers,
        }
    }

    async fn create_stream_async(&self) -> Result<AsyncReadOnlySource, AudioStreamError> {
        let request = self
            .client
            .get(&self.request)
            .headers(self.headers.clone())
            .build()
            .map_err(|why| AudioStreamError::Fail(why.into()))?;

        let response = self
            .client
            .execute(request)
            .await
            .map_err(|why| AudioStreamError::Fail(why.into()))?;

        if !response.status().is_success() {
            return Err(AudioStreamError::Fail(
                format!("HTTP error: {}", response.status()).into(),
            ));
        }

        let byte_stream = response
            .bytes_stream()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));

        let tokio_reader = byte_stream.into_async_read().compat();

        Ok(AsyncReadOnlySource::new(tokio_reader))
    }
}

#[async_trait]
impl Compose for Mp3Request {
    fn create(&mut self) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Fail(
            "Mp3Request::create must be called in an async context via create_async".into(),
        ))
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError> {
        let input = self.create_stream_async().await?;
        let stream = AsyncAdapterStream::new(Box::new(input), 64 * 1024);

        let hint = Hint::new().with_extension("mp3").clone();
        Ok(AudioStream {
            input: Box::new(stream) as Box<dyn MediaSource>,
            hint: Some(hint),
        })
    }

    fn should_create_async(&self) -> bool {
        true
    }
}

impl From<Mp3Request> for Input {
    fn from(val: Mp3Request) -> Self {
        Input::Lazy(Box::new(val))
    }
}
