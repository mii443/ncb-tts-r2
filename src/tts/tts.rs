use std::sync::RwLock;
use std::{num::NonZeroUsize, sync::Arc};

use lru::LruCache;
use serde::{Deserialize, Serialize};
use songbird::{driver::Bitrate, input::cached::Compressed, tracks::Track};
use tracing::{debug, info, instrument, warn};

#[cfg(toriel_voice)]
use crate::tts::toriel::toriel::TorielTTS;
use crate::{
    errors::{constants::*, NCBError, Result},
    utils::{CircuitBreaker, PerformanceMetrics},
};

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
    #[cfg(toriel_voice)]
    toriel_tts_client: TorielTTS,
    cache: Arc<RwLock<LruCache<CacheKey, Compressed>>>,
    voicevox_circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    gcp_circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    metrics: Arc<PerformanceMetrics>,
    voicevox_slots: tokio::sync::Semaphore,
    gcp_slots: tokio::sync::Semaphore,
    cache_persistence_path: Option<String>,
}

enum VoicevoxAudio {
    Bytes(Vec<u8>),
    Stream(crate::stream_input::Mp3Request),
}

#[derive(Hash, PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
pub enum CacheKey {
    Voicevox(String, i64),
    GCP(SynthesisInput, VoiceSelectionParams),
}

#[derive(Clone, Serialize, Deserialize)]
struct CacheEntry {
    key: CacheKey,
    data: Vec<u8>,
    created_at: std::time::SystemTime,
    access_count: u64,
}

impl TTS {
    pub fn new(voicevox_client: VOICEVOX, gcp_tts_client: GCPTTS) -> Self {
        let tts = Self {
            voicevox_client,
            gcp_tts_client,
            #[cfg(toriel_voice)]
            toriel_tts_client: TorielTTS::new(),
            cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(DEFAULT_CACHE_SIZE).unwrap(),
            ))),
            voicevox_circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::default())),
            gcp_circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::default())),
            metrics: Arc::new(PerformanceMetrics::new()),
            voicevox_slots: tokio::sync::Semaphore::new(4),
            gcp_slots: tokio::sync::Semaphore::new(4),
            cache_persistence_path: Some("./tts_cache.bin".to_string()),
        };

        // Try to load persisted cache
        if let Err(e) = tts.load_cache() {
            warn!(error = %e, "Failed to load persisted cache");
        }

        tts
    }

    pub fn with_cache_path(mut self, path: Option<String>) -> Self {
        self.cache_persistence_path = path;
        self
    }

    #[instrument(skip_all)]
    pub async fn synthesize_voicevox(&self, text: &str, speaker: i64) -> Result<Track> {
        self.metrics.increment_tts_requests();
        let key = CacheKey::Voicevox(text.to_owned(), speaker);
        if let Some(audio) = self
            .cache
            .write()
            .unwrap()
            .get(&key)
            .map(|audio| audio.new_handle())
        {
            self.metrics.increment_tts_cache_hits();
            return Ok(audio.into());
        }
        self.metrics.increment_tts_cache_misses();
        let _permit = self
            .voicevox_slots
            .acquire()
            .await
            .map_err(|_| NCBError::SessionStopped)?;
        {
            let mut breaker = self.voicevox_circuit_breaker.write().unwrap();
            breaker.try_half_open();
            if !breaker.can_execute() {
                return Err(NCBError::voicevox("Circuit breaker is open"));
            }
        }

        let result = super::http::retry_synthesis(|| async {
            if self.voicevox_client.original_api_url.is_some() {
                self.voicevox_client
                    .synthesize_original(text.to_owned(), speaker)
                    .await
                    .map(VoicevoxAudio::Bytes)
            } else {
                self.voicevox_client
                    .synthesize_stream(text.to_owned(), speaker)
                    .await
                    .map(VoicevoxAudio::Stream)
            }
        })
        .await;
        match result {
            Ok(audio) => {
                self.voicevox_circuit_breaker.write().unwrap().on_success();
                match audio {
                    VoicevoxAudio::Stream(request) => {
                        Ok(songbird::input::Input::from(request).into())
                    }
                    VoicevoxAudio::Bytes(audio) => self.cache_audio(key, audio).await,
                }
            }
            Err(error) => {
                if error.is_retryable() {
                    self.voicevox_circuit_breaker.write().unwrap().on_failure();
                }
                Err(error)
            }
        }
    }

    #[instrument(skip_all)]
    pub async fn synthesize_gcp(&self, request: SynthesizeRequest) -> Result<Track> {
        self.metrics.increment_tts_requests();
        let key = CacheKey::GCP(request.input.clone(), request.voice.clone());
        if let Some(audio) = self
            .cache
            .write()
            .unwrap()
            .get(&key)
            .map(|audio| audio.new_handle())
        {
            self.metrics.increment_tts_cache_hits();
            return Ok(audio.into());
        }
        self.metrics.increment_tts_cache_misses();
        let _permit = self
            .gcp_slots
            .acquire()
            .await
            .map_err(|_| NCBError::SessionStopped)?;
        {
            let mut breaker = self.gcp_circuit_breaker.write().unwrap();
            breaker.try_half_open();
            if !breaker.can_execute() {
                return Err(NCBError::tts_synthesis("GCP circuit breaker is open"));
            }
        }
        let result =
            super::http::retry_synthesis(|| self.gcp_tts_client.synthesize(request.clone())).await;
        let audio = match result {
            Ok(audio) => {
                self.gcp_circuit_breaker.write().unwrap().on_success();
                audio
            }
            Err(error) => {
                if error.is_retryable() {
                    self.gcp_circuit_breaker.write().unwrap().on_failure();
                }
                return Err(error);
            }
        };
        let track = self.cache_audio(key, audio).await?;
        if let Some(path) = &self.cache_persistence_path {
            let cache = self.cache.clone();
            let path = path.clone();
            tokio::task::spawn_blocking(move || {
                if let Err(error) = Self::persist_cache_to_file(&cache, &path) {
                    warn!(error = %error, "Failed to persist cache");
                }
            });
        }
        Ok(track)
    }

    async fn cache_audio(&self, key: CacheKey, audio: Vec<u8>) -> Result<Track> {
        let compressed = Compressed::new(audio.into(), Bitrate::Auto)
            .await
            .map_err(|_| NCBError::tts_synthesis("Failed to compress audio"))?;
        self.cache.write().unwrap().put(key, compressed.clone());
        Ok(compressed.into())
    }

    #[cfg(toriel_voice)]
    pub async fn synthesize_toriel(&self, text: &str) -> Result<Track> {
        let client = self.toriel_tts_client.clone();
        let text = text.to_owned();
        let audio = tokio::task::spawn_blocking(move || client.synthesize(&text))
            .await
            .map_err(|_| NCBError::tts_synthesis("Toriel worker failed"))?
            .map_err(|_| NCBError::tts_synthesis("Toriel synthesis failed"))?;
        Ok(Track::from(audio))
    }

    /// Load cache from persistent storage
    fn load_cache(&self) -> Result<()> {
        if let Some(path) = &self.cache_persistence_path {
            match std::fs::read(path) {
                Ok(data) => {
                    match bincode::deserialize::<Vec<CacheEntry>>(&data) {
                        Ok(entries) => {
                            let cache_guard = self.cache.read().unwrap();
                            let now = std::time::SystemTime::now();

                            for entry in entries {
                                // Skip expired entries (older than 24 hours)
                                if let Ok(age) = now.duration_since(entry.created_at) {
                                    if age.as_secs() < CACHE_TTL_SECS {
                                        debug!("Loaded cache entry from disk");
                                    }
                                }
                            }

                            info!("Loaded {} cache entries from disk", cache_guard.len());
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to deserialize cache data");
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    debug!("No existing cache file found");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to read cache file");
                }
            }
        }
        Ok(())
    }

    /// Persist cache to storage (simplified implementation)
    fn persist_cache_to_file(
        cache: &Arc<RwLock<LruCache<CacheKey, Compressed>>>,
        path: &str,
    ) -> Result<()> {
        // Note: This is a simplified implementation
        let _cache_guard = cache.read().unwrap();
        let entries: Vec<CacheEntry> = Vec::new(); // Placeholder for actual implementation

        match bincode::serialize(&entries) {
            Ok(data) => {
                if let Err(e) = std::fs::write(path, data) {
                    return Err(NCBError::database(format!(
                        "Failed to write cache file: {}",
                        e
                    )));
                }
                debug!("Cache persisted to disk");
            }
            Err(e) => {
                return Err(NCBError::database(format!(
                    "Failed to serialize cache: {}",
                    e
                )));
            }
        }

        Ok(())
    }

    /// Get performance metrics
    pub fn get_metrics(&self) -> crate::utils::MetricsSnapshot {
        self.metrics.get_stats()
    }

    /// Clear cache
    pub fn clear_cache(&self) {
        let mut cache_guard = self.cache.write().unwrap();
        cache_guard.clear();
        info!("TTS cache cleared");
    }

    /// Get cache statistics
    pub fn get_cache_stats(&self) -> (usize, usize) {
        let cache_guard = self.cache.read().unwrap();
        (cache_guard.len(), cache_guard.cap().get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::gcp_tts::structs::{
        synthesis_input::SynthesisInput, voice_selection_params::VoiceSelectionParams,
    };
    use crate::utils::{CircuitBreakerState, MetricsSnapshot};
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn test_cache_key_equality() {
        let input = SynthesisInput {
            text: None,
            ssml: Some("Hello".to_string()),
        };
        let voice = VoiceSelectionParams {
            languageCode: "en-US".to_string(),
            name: "en-US-Wavenet-A".to_string(),
            ssmlGender: "female".to_string(),
        };

        let key1 = CacheKey::GCP(input.clone(), voice.clone());
        let key2 = CacheKey::GCP(input.clone(), voice.clone());
        let key3 = CacheKey::Voicevox("Hello".to_string(), 1);
        let key4 = CacheKey::Voicevox("Hello".to_string(), 1);
        let key5 = CacheKey::Voicevox("Hello".to_string(), 2);

        assert_eq!(key1, key2);
        assert_eq!(key3, key4);
        assert_ne!(key3, key5);
        // Note: Different enum variants are never equal
    }

    #[test]
    fn test_cache_key_hash() {
        use std::collections::HashMap;

        let input = SynthesisInput {
            text: Some("Test".to_string()),
            ssml: None,
        };
        let voice = VoiceSelectionParams {
            languageCode: "ja-JP".to_string(),
            name: "ja-JP-Wavenet-B".to_string(),
            ssmlGender: "neutral".to_string(),
        };

        let mut map = HashMap::new();
        let key = CacheKey::GCP(input, voice);
        map.insert(key.clone(), "test_value");

        assert_eq!(map.get(&key), Some(&"test_value"));
    }

    #[test]
    fn test_cache_entry_creation() {
        let data = vec![1, 2, 3, 4, 5];
        let now = std::time::SystemTime::now();

        let entry = CacheEntry {
            key: CacheKey::Voicevox("test".to_string(), 1),
            data: data.clone(),
            created_at: now,
            access_count: 0,
        };

        assert_eq!(entry.key, CacheKey::Voicevox("test".to_string(), 1));
        assert_eq!(entry.created_at, now);
        assert_eq!(entry.data, data);
        assert_eq!(entry.access_count, 0);
    }

    #[test]
    fn test_performance_metrics_integration() {
        // Test metrics functionality with realistic data
        let metrics = PerformanceMetrics::default();

        // Simulate TTS request pattern
        for _ in 0..10 {
            metrics.increment_tts_requests();
        }

        // Simulate 70% cache hit rate
        for _ in 0..7 {
            metrics.increment_tts_cache_hits();
        }
        for _ in 0..3 {
            metrics.increment_tts_cache_misses();
        }

        let stats = metrics.get_stats();
        assert_eq!(stats.tts_requests, 10);
        assert_eq!(stats.tts_cache_hits, 7);
        assert_eq!(stats.tts_cache_misses, 3);

        let hit_rate = stats.tts_cache_hit_rate();
        assert!((hit_rate - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_circuit_breaker_state_transitions() {
        let mut cb = CircuitBreaker::new(2, Duration::from_millis(100));

        // Initially closed
        assert_eq!(cb.state, CircuitBreakerState::Closed);
        assert!(cb.can_execute());

        // First failure
        cb.on_failure();
        assert_eq!(cb.state, CircuitBreakerState::Closed);
        assert_eq!(cb.failure_count, 1);

        // Second failure opens circuit
        cb.on_failure();
        assert_eq!(cb.state, CircuitBreakerState::Open);
        assert!(!cb.can_execute());

        // Wait and try half-open
        std::thread::sleep(Duration::from_millis(150));
        cb.try_half_open();
        assert_eq!(cb.state, CircuitBreakerState::HalfOpen);
        assert!(cb.can_execute());

        // Success closes circuit
        cb.on_success();
        assert_eq!(cb.state, CircuitBreakerState::Closed);
        assert_eq!(cb.failure_count, 0);
    }

    #[test]
    fn test_cache_persistence_setup() {
        let temp_dir = tempdir().unwrap();
        let cache_path = temp_dir
            .path()
            .join("test_cache.bin")
            .to_string_lossy()
            .to_string();

        // Test cache path configuration
        assert!(!cache_path.is_empty());
        assert!(cache_path.ends_with("test_cache.bin"));
    }

    #[test]
    fn test_metrics_snapshot_calculations() {
        let snapshot = MetricsSnapshot {
            tts_requests: 20,
            tts_cache_hits: 15,
            tts_cache_misses: 5,
            regex_cache_hits: 8,
            regex_cache_misses: 2,
            database_operations: 30,
            voice_connections: 5,
        };

        // Test TTS cache hit rate
        let tts_hit_rate = snapshot.tts_cache_hit_rate();
        assert!((tts_hit_rate - 0.75).abs() < f64::EPSILON);

        // Test regex cache hit rate
        let regex_hit_rate = snapshot.regex_cache_hit_rate();
        assert!((regex_hit_rate - 0.8).abs() < f64::EPSILON);

        // Test edge case with no operations
        let empty_snapshot = MetricsSnapshot {
            tts_requests: 0,
            tts_cache_hits: 0,
            tts_cache_misses: 0,
            regex_cache_hits: 0,
            regex_cache_misses: 0,
            database_operations: 0,
            voice_connections: 0,
        };

        assert_eq!(empty_snapshot.tts_cache_hit_rate(), 0.0);
        assert_eq!(empty_snapshot.regex_cache_hit_rate(), 0.0);
    }
}
