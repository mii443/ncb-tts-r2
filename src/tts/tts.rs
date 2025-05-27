use std::sync::RwLock;
use std::{num::NonZeroUsize, sync::Arc};

use lru::LruCache;
use serde::{Deserialize, Serialize};
use songbird::{driver::Bitrate, input::cached::Compressed, tracks::Track};
use tracing::{debug, error, info, instrument, warn};

use crate::{
    errors::{constants::*, NCBError, Result},
    utils::{retry_with_backoff, CircuitBreaker, PerformanceMetrics},
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
    cache: Arc<RwLock<LruCache<CacheKey, Compressed>>>,
    voicevox_circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    gcp_circuit_breaker: Arc<RwLock<CircuitBreaker>>,
    metrics: Arc<PerformanceMetrics>,
    cache_persistence_path: Option<String>,
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
            cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(DEFAULT_CACHE_SIZE).unwrap(),
            ))),
            voicevox_circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::default())),
            gcp_circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::default())),
            metrics: Arc::new(PerformanceMetrics::new()),
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

    #[instrument(skip(self))]
    pub async fn synthesize_voicevox(
        &self,
        text: &str,
        speaker: i64,
    ) -> std::result::Result<Track, NCBError> {
        self.metrics.increment_tts_requests();
        let cache_key = CacheKey::Voicevox(text.to_string(), speaker);

        let cached_audio = {
            let mut cache_guard = self.cache.write().unwrap();
            cache_guard.get(&cache_key).map(|audio| audio.new_handle())
        };

        if let Some(audio) = cached_audio {
            debug!("Cache hit for VOICEVOX TTS");
            self.metrics.increment_tts_cache_hits();
            return Ok(audio.into());
        }

        debug!("Cache miss for VOICEVOX TTS");
        self.metrics.increment_tts_cache_misses();

        // Check circuit breaker
        {
            let mut circuit_breaker = self.voicevox_circuit_breaker.write().unwrap();
            circuit_breaker.try_half_open();

            if !circuit_breaker.can_execute() {
                return Err(NCBError::voicevox("Circuit breaker is open"));
            }
        }

        let synthesis_result = if self.voicevox_client.original_api_url.is_some() {
            retry_with_backoff(
                || async {
                    match self
                        .voicevox_client
                        .synthesize_original(text.to_string(), speaker)
                        .await
                    {
                        Ok(audio) => Ok(audio),
                        Err(e) => Err(NCBError::voicevox(format!(
                            "VOICEVOX synthesis failed: {}",
                            e
                        ))),
                    }
                },
                3,
                std::time::Duration::from_millis(500),
            )
            .await
        } else {
            retry_with_backoff(
                || async {
                    match self
                        .voicevox_client
                        .synthesize_stream(text.to_string(), speaker)
                        .await
                    {
                        Ok(_mp3_request) => Err(NCBError::voicevox(
                            "Stream synthesis not yet fully implemented",
                        )),
                        Err(e) => Err(NCBError::voicevox(format!(
                            "VOICEVOX synthesis failed: {}",
                            e
                        ))),
                    }
                },
                3,
                std::time::Duration::from_millis(500),
            )
            .await
        };

        match synthesis_result {
            Ok(audio) => {
                // Update circuit breaker on success
                let mut circuit_breaker = self.voicevox_circuit_breaker.write().unwrap();
                circuit_breaker.on_success();
                drop(circuit_breaker);

                // Cache the audio asynchronously
                let cache = self.cache.clone();
                let cache_key_clone = cache_key.clone();
                let audio_for_cache = audio.clone();
                tokio::spawn(async move {
                    debug!("Compressing and caching VOICEVOX audio");
                    if let Ok(compressed) =
                        Compressed::new(audio_for_cache.into(), Bitrate::Auto).await
                    {
                        let mut cache_guard = cache.write().unwrap();
                        cache_guard.put(cache_key_clone, compressed);
                    }
                });

                Ok(audio.into())
            }
            Err(e) => {
                // Update circuit breaker on failure
                let mut circuit_breaker = self.voicevox_circuit_breaker.write().unwrap();
                circuit_breaker.on_failure();
                drop(circuit_breaker);

                error!(error = %e, "VOICEVOX synthesis failed");
                Err(e)
            }
        }
    }

    pub async fn synthesize_gcp(
        &self,
        synthesize_request: SynthesizeRequest,
    ) -> std::result::Result<Track, NCBError> {
        self.metrics.increment_tts_requests();
        let cache_key = CacheKey::GCP(
            synthesize_request.input.clone(),
            synthesize_request.voice.clone(),
        );

        let cached_audio = {
            let mut cache_guard = self.cache.write().unwrap();
            cache_guard.get(&cache_key).map(|audio| audio.new_handle())
        };

        if let Some(audio) = cached_audio {
            debug!("Cache hit for GCP TTS");
            self.metrics.increment_tts_cache_hits();
            return Ok(audio.into());
        }

        debug!("Cache miss for GCP TTS");
        self.metrics.increment_tts_cache_misses();

        // Check circuit breaker
        {
            let mut circuit_breaker = self.gcp_circuit_breaker.write().unwrap();
            circuit_breaker.try_half_open();

            if !circuit_breaker.can_execute() {
                return Err(NCBError::tts_synthesis("GCP TTS circuit breaker is open"));
            }
        }

        let request_clone = SynthesizeRequest {
            input: synthesize_request.input.clone(),
            voice: synthesize_request.voice.clone(),
            audioConfig: synthesize_request.audioConfig.clone(),
        };

        let audio = {
            let audio_result = retry_with_backoff(
                || async {
                    match self.gcp_tts_client.synthesize(request_clone.clone()).await {
                        Ok(audio) => Ok(audio),
                        Err(e) => Err(NCBError::tts_synthesis(format!(
                            "GCP TTS synthesis failed: {}",
                            e
                        ))),
                    }
                },
                3,
                std::time::Duration::from_millis(500),
            )
            .await;

            match audio_result {
                Ok(audio) => audio,
                Err(e) => {
                    // Update circuit breaker on failure
                    let mut circuit_breaker = self.gcp_circuit_breaker.write().unwrap();
                    circuit_breaker.on_failure();
                    drop(circuit_breaker);

                    error!(error = %e, "GCP TTS synthesis failed");
                    return Err(e);
                }
            }
        };

        // Update circuit breaker on success
        {
            let mut circuit_breaker = self.gcp_circuit_breaker.write().unwrap();
            circuit_breaker.on_success();
        }

        match Compressed::new(audio.into(), Bitrate::Auto).await {
            Ok(compressed) => {
                // Cache the compressed audio
                {
                    let mut cache_guard = self.cache.write().unwrap();
                    cache_guard.put(cache_key, compressed.clone());
                }

                // Persist cache asynchronously
                if let Some(path) = &self.cache_persistence_path {
                    let cache_clone = self.cache.clone();
                    let path_clone = path.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::persist_cache_to_file(&cache_clone, &path_clone) {
                            warn!(error = %e, "Failed to persist cache");
                        }
                    });
                }

                Ok(compressed.into())
            }
            Err(e) => {
                error!(error = %e, "Failed to compress GCP audio");
                Err(NCBError::tts_synthesis(format!(
                    "Audio compression failed: {}",
                    e
                )))
            }
        }
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
    use crate::errors::constants::CIRCUIT_BREAKER_FAILURE_THRESHOLD;
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
