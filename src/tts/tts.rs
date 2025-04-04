use std::num::NonZeroUsize;

use lru::LruCache;
use songbird::{driver::Bitrate, input::cached::Compressed};
use tracing::info;

use super::{gcp_tts::{gcp_tts::GCPTTS, structs::{synthesis_input::SynthesisInput, synthesize_request::SynthesizeRequest, voice_selection_params::VoiceSelectionParams}}, voicevox::voicevox::VOICEVOX};

pub struct TTS {
    pub voicevox_client: VOICEVOX,
    gcp_tts_client: GCPTTS,
    cache: LruCache<CacheKey, Compressed>,
}

#[derive(Hash, PartialEq, Eq)]
pub enum CacheKey {
    Voicevox(String, i64),
    GCP(SynthesisInput, VoiceSelectionParams),
}

impl TTS {
    pub fn new(
        voicevox_client: VOICEVOX,
        gcp_tts_client: GCPTTS,
    ) -> Self {
        Self {
            voicevox_client,
            gcp_tts_client,
            cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
        }
    }

    pub async fn synthesize_voicevox(&mut self, text: &str, speaker: i64) -> Result<Compressed, Box<dyn std::error::Error>> {
        let cache_key = CacheKey::Voicevox(text.to_string(), speaker);

        if let Some(audio) = self.cache.get(&cache_key) {
            info!("Cache hit for VOICEVOX TTS");
            return Ok(audio.new_handle());
        }
        info!("Cache miss for VOICEVOX TTS");

        let audio = self.voicevox_client
            .synthesize(text.to_string(), speaker)
            .await?;

        let compressed = Compressed::new(audio.into(), Bitrate::Auto).await?;
        
        self.cache.put(cache_key, compressed.clone());

        Ok(compressed)
    }

    pub async fn synthesize_gcp(&mut self, synthesize_request: SynthesizeRequest) -> Result<Compressed, Box<dyn std::error::Error>> {
        let cache_key = CacheKey::GCP(
            synthesize_request.input.clone(),
            synthesize_request.voice.clone(),
        );

        if let Some(audio) = self.cache.get(&cache_key) {
            info!("Cache hit for GCP TTS");
            return Ok(audio.new_handle());
        }
        info!("Cache miss for GCP TTS");

        let audio = self.gcp_tts_client
            .synthesize(synthesize_request)
            .await?;

        let compressed = Compressed::new(audio.into(), Bitrate::Auto).await?;
    
        self.cache.put(cache_key, compressed.clone());

        Ok(compressed)
    }
}
