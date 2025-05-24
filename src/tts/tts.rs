use std::sync::RwLock;
use std::{num::NonZeroUsize, sync::Arc};

use lru::LruCache;
use songbird::{driver::Bitrate, input::cached::Compressed, tracks::Track};
use tracing::info;

use super::{
    gcp_tts::{
        gcp_tts::GCPTTS,
        structs::{
            synthesis_input::SynthesisInput, synthesize_request::SynthesizeRequest,
            voice_selection_params::VoiceSelectionParams,
        },
    },
    voicevox::voicevox::VOICEVOX,
};

#[derive(Debug)]
pub struct TTS {
    pub voicevox_client: VOICEVOX,
    gcp_tts_client: GCPTTS,
    cache: Arc<RwLock<LruCache<CacheKey, Compressed>>>,
}

#[derive(Hash, PartialEq, Eq)]
pub enum CacheKey {
    Voicevox(String, i64),
    GCP(SynthesisInput, VoiceSelectionParams),
}

impl TTS {
    pub fn new(voicevox_client: VOICEVOX, gcp_tts_client: GCPTTS) -> Self {
        Self {
            voicevox_client,
            gcp_tts_client,
            cache: Arc::new(RwLock::new(LruCache::new(NonZeroUsize::new(1000).unwrap()))),
        }
    }

    #[tracing::instrument]
    pub async fn synthesize_voicevox(
        &self,
        text: &str,
        speaker: i64,
    ) -> Result<Track, Box<dyn std::error::Error>> {
        let cache_key = CacheKey::Voicevox(text.to_string(), speaker);

        let cached_audio = {
            let mut cache_guard = self.cache.write().unwrap();
            cache_guard.get(&cache_key).map(|audio| audio.new_handle())
        };

        if let Some(audio) = cached_audio {
            info!("Cache hit for VOICEVOX TTS");
            return Ok(audio.into());
        }

        info!("Cache miss for VOICEVOX TTS");

        if self.voicevox_client.original_api_url.is_some() {
            let audio = self
                .voicevox_client
                .synthesize_original(text.to_string(), speaker)
                .await?;

            tokio::spawn({
                let cache = self.cache.clone();
                let audio = audio.clone();
                async move {
                    info!("Compressing stream audio");
                    let compressed = Compressed::new(audio.into(), Bitrate::Auto).await.unwrap();
                    let mut cache_guard = cache.write().unwrap();
                    cache_guard.put(cache_key, compressed.clone());
                }
            });

            Ok(audio.into())
        } else {
            let audio = self
                .voicevox_client
                .synthesize_stream(text.to_string(), speaker)
                .await?;

            tokio::spawn({
                let cache = self.cache.clone();
                let audio = audio.clone();
                async move {
                    info!("Compressing stream audio");
                    let compressed = Compressed::new(audio.into(), Bitrate::Auto).await.unwrap();
                    let mut cache_guard = cache.write().unwrap();
                    cache_guard.put(cache_key, compressed.clone());
                }
            });

            Ok(audio.into())
        }
    }

    #[tracing::instrument]
    pub async fn synthesize_gcp(
        &self,
        synthesize_request: SynthesizeRequest,
    ) -> Result<Compressed, Box<dyn std::error::Error>> {
        let cache_key = CacheKey::GCP(
            synthesize_request.input.clone(),
            synthesize_request.voice.clone(),
        );

        let cached_audio = {
            let mut cache_guard = self.cache.write().unwrap();
            cache_guard.get(&cache_key).map(|audio| audio.new_handle())
        };

        if let Some(audio) = cached_audio {
            info!("Cache hit for GCP TTS");
            return Ok(audio);
        }

        info!("Cache miss for GCP TTS");

        let audio = self.gcp_tts_client.synthesize(synthesize_request).await?;

        let compressed = Compressed::new(audio.into(), Bitrate::Auto).await?;

        {
            let mut cache_guard = self.cache.write().unwrap();
            cache_guard.put(cache_key, compressed.clone());
        }

        Ok(compressed)
    }
}
