use once_cell::sync::Lazy;
use lru::LruCache;
use regex::Regex;
use std::{num::NonZeroUsize, sync::RwLock};
use tracing::{debug, error, warn};

use crate::errors::{constants::*, NCBError, Result};

/// Regex compilation cache to avoid recompiling the same patterns
static REGEX_CACHE: Lazy<RwLock<LruCache<String, Regex>>> = 
    Lazy::new(|| RwLock::new(LruCache::new(NonZeroUsize::new(DEFAULT_CACHE_SIZE).unwrap())));

/// Circuit breaker states for external API calls
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Circuit breaker for handling external API failures
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    pub state: CircuitBreakerState,
    pub failure_count: u32,
    pub last_failure_time: Option<std::time::Instant>,
    pub threshold: u32,
    pub timeout: std::time::Duration,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            last_failure_time: None,
            threshold: 5,
            timeout: std::time::Duration::from_secs(60),
        }
    }
}

impl CircuitBreaker {
    pub fn new(threshold: u32, timeout: std::time::Duration) -> Self {
        Self {
            threshold,
            timeout,
            ..Default::default()
        }
    }

    pub fn can_execute(&self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    last_failure.elapsed() >= self.timeout
                } else {
                    true
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    pub fn on_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitBreakerState::Closed;
        self.last_failure_time = None;
    }

    pub fn on_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_time = Some(std::time::Instant::now());

        if self.failure_count >= self.threshold {
            self.state = CircuitBreakerState::Open;
        } else if self.state == CircuitBreakerState::HalfOpen {
            self.state = CircuitBreakerState::Open;
        }
    }

    pub fn try_half_open(&mut self) {
        if self.state == CircuitBreakerState::Open {
            if let Some(last_failure) = self.last_failure_time {
                if last_failure.elapsed() >= self.timeout {
                    self.state = CircuitBreakerState::HalfOpen;
                }
            }
        }
    }
}

/// Cached regex compilation with error handling
pub fn get_cached_regex(pattern: &str) -> Result<Regex> {
    // First try to get from cache
    {
        let cache = REGEX_CACHE.read().unwrap();
        if let Some(cached_regex) = cache.peek(pattern) {
            debug!(pattern = pattern, "Regex cache hit");
            return Ok(cached_regex.clone());
        }
    }

    debug!(pattern = pattern, "Regex cache miss, compiling");

    // Compile regex with error handling
    match Regex::new(pattern) {
        Ok(regex) => {
            // Cache successful compilation
            {
                let mut cache = REGEX_CACHE.write().unwrap();
                cache.put(pattern.to_string(), regex.clone());
            }
            Ok(regex)
        }
        Err(e) => {
            error!(pattern = pattern, error = %e, "Failed to compile regex");
            Err(NCBError::invalid_regex(format!("{}: {}", pattern, e)))
        }
    }
}

/// Retry logic with exponential backoff
pub async fn retry_with_backoff<F, Fut, T, E>(
    mut operation: F,
    max_attempts: u32,
    initial_delay: std::time::Duration,
) -> std::result::Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempts = 0;
    let mut delay = initial_delay;

    loop {
        attempts += 1;
        
        match operation().await {
            Ok(result) => {
                if attempts > 1 {
                    debug!(attempts = attempts, "Operation succeeded after retry");
                }
                return Ok(result);
            }
            Err(error) => {
                if attempts >= max_attempts {
                    error!(
                        attempts = attempts,
                        error = %error,
                        "Operation failed after maximum retry attempts"
                    );
                    return Err(error);
                }

                warn!(
                    attempt = attempts,
                    max_attempts = max_attempts,
                    delay_ms = delay.as_millis(),
                    error = %error,
                    "Operation failed, retrying with backoff"
                );

                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, std::time::Duration::from_secs(30));
            }
        }
    }
}

/// Rate limiter using token bucket algorithm
#[derive(Debug)]
pub struct RateLimiter {
    tokens: std::sync::Arc<std::sync::RwLock<f64>>,
    capacity: f64,
    refill_rate: f64,
    last_refill: std::sync::Arc<std::sync::RwLock<std::time::Instant>>,
}

impl RateLimiter {
    pub fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: std::sync::Arc::new(std::sync::RwLock::new(capacity)),
            capacity,
            refill_rate,
            last_refill: std::sync::Arc::new(std::sync::RwLock::new(std::time::Instant::now())),
        }
    }

    pub fn try_acquire(&self, tokens: f64) -> bool {
        self.refill();
        
        let mut current_tokens = self.tokens.write().unwrap();
        if *current_tokens >= tokens {
            *current_tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn refill(&self) {
        let now = std::time::Instant::now();
        let mut last_refill = self.last_refill.write().unwrap();
        let elapsed = now.duration_since(*last_refill).as_secs_f64();
        
        if elapsed > 0.0 {
            let tokens_to_add = elapsed * self.refill_rate;
            let mut current_tokens = self.tokens.write().unwrap();
            *current_tokens = (*current_tokens + tokens_to_add).min(self.capacity);
            *last_refill = now;
        }
    }
}

/// Performance metrics collection
#[derive(Debug, Default, Clone)]
pub struct PerformanceMetrics {
    pub tts_requests: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub tts_cache_hits: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub tts_cache_misses: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub regex_cache_hits: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub regex_cache_misses: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub database_operations: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub voice_connections: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment_tts_requests(&self) {
        self.tts_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_tts_cache_hits(&self) {
        self.tts_cache_hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_tts_cache_misses(&self) {
        self.tts_cache_misses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_regex_cache_hits(&self) {
        self.regex_cache_hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_regex_cache_misses(&self) {
        self.regex_cache_misses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_database_operations(&self) {
        self.database_operations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn increment_voice_connections(&self) {
        self.voice_connections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            tts_requests: self.tts_requests.load(std::sync::atomic::Ordering::Relaxed),
            tts_cache_hits: self.tts_cache_hits.load(std::sync::atomic::Ordering::Relaxed),
            tts_cache_misses: self.tts_cache_misses.load(std::sync::atomic::Ordering::Relaxed),
            regex_cache_hits: self.regex_cache_hits.load(std::sync::atomic::Ordering::Relaxed),
            regex_cache_misses: self.regex_cache_misses.load(std::sync::atomic::Ordering::Relaxed),
            database_operations: self.database_operations.load(std::sync::atomic::Ordering::Relaxed),
            voice_connections: self.voice_connections.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub tts_requests: u64,
    pub tts_cache_hits: u64,
    pub tts_cache_misses: u64,
    pub regex_cache_hits: u64,
    pub regex_cache_misses: u64,
    pub database_operations: u64,
    pub voice_connections: u64,
}

impl MetricsSnapshot {
    pub fn tts_cache_hit_rate(&self) -> f64 {
        if self.tts_cache_hits + self.tts_cache_misses > 0 {
            self.tts_cache_hits as f64 / (self.tts_cache_hits + self.tts_cache_misses) as f64
        } else {
            0.0
        }
    }

    pub fn regex_cache_hit_rate(&self) -> f64 {
        if self.regex_cache_hits + self.regex_cache_misses > 0 {
            self.regex_cache_hits as f64 / (self.regex_cache_hits + self.regex_cache_misses) as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use crate::errors::constants::CIRCUIT_BREAKER_FAILURE_THRESHOLD;
    
    #[test]
    fn test_circuit_breaker_default() {
        let cb = CircuitBreaker::default();
        assert_eq!(cb.state, CircuitBreakerState::Closed);
        assert_eq!(cb.failure_count, 0);
        assert!(cb.can_execute());
    }
    
    #[test]
    fn test_circuit_breaker_new() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(10));
        assert_eq!(cb.state, CircuitBreakerState::Closed);
        assert_eq!(cb.threshold, 3);
        assert_eq!(cb.timeout, Duration::from_secs(10));
    }
    
    #[test]
    fn test_circuit_breaker_failure_threshold() {
        let mut cb = CircuitBreaker::default();
        
        // Test failures up to threshold
        for i in 0..CIRCUIT_BREAKER_FAILURE_THRESHOLD {
            assert_eq!(cb.state, CircuitBreakerState::Closed);
            assert!(cb.can_execute());
            cb.on_failure();
            assert_eq!(cb.failure_count, i + 1);
        }
        
        // Should open after reaching threshold
        assert_eq!(cb.state, CircuitBreakerState::Open);
        assert!(!cb.can_execute());
    }
    
    #[test]
    fn test_circuit_breaker_success_resets() {
        let mut cb = CircuitBreaker::default();
        
        // Add some failures
        cb.on_failure();
        cb.on_failure();
        assert_eq!(cb.failure_count, 2);
        
        // Success should reset
        cb.on_success();
        assert_eq!(cb.failure_count, 0);
        assert_eq!(cb.state, CircuitBreakerState::Closed);
    }
    
    #[test]
    fn test_circuit_breaker_half_open() {
        let mut cb = CircuitBreaker::new(1, Duration::from_millis(100));
        
        // Trigger failure to open circuit
        cb.on_failure();
        assert_eq!(cb.state, CircuitBreakerState::Open);
        assert!(!cb.can_execute());
        
        // Wait for timeout
        std::thread::sleep(Duration::from_millis(150));
        
        // Should allow transition to half-open
        cb.try_half_open();
        assert_eq!(cb.state, CircuitBreakerState::HalfOpen);
        assert!(cb.can_execute());
        
        // Success in half-open should close circuit
        cb.on_success();
        assert_eq!(cb.state, CircuitBreakerState::Closed);
    }
    
    #[test]
    fn test_circuit_breaker_half_open_failure() {
        let mut cb = CircuitBreaker::new(1, Duration::from_millis(100));
        
        // Open circuit
        cb.on_failure();
        std::thread::sleep(Duration::from_millis(150));
        cb.try_half_open();
        assert_eq!(cb.state, CircuitBreakerState::HalfOpen);
        
        // Failure in half-open should reopen circuit
        cb.on_failure();
        assert_eq!(cb.state, CircuitBreakerState::Open);
        assert!(!cb.can_execute());
    }
    
    #[tokio::test]
    async fn test_retry_with_backoff_success_first_try() {
        let mut call_count = 0;
        let result = retry_with_backoff(
            || {
                call_count += 1;
                async { Ok::<i32, &'static str>(42) }
            },
            3,
            Duration::from_millis(100),
        ).await;
        
        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count, 1);
    }
    
    #[tokio::test]
    async fn test_retry_with_backoff_success_after_retries() {
        let mut call_count = 0;
        let result = retry_with_backoff(
            || {
                call_count += 1;
                async move {
                    if call_count < 3 {
                        Err("temporary error")
                    } else {
                        Ok::<i32, &'static str>(42)
                    }
                }
            },
            5,
            Duration::from_millis(10),
        ).await;
        
        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count, 3);
    }
    
    #[tokio::test]
    async fn test_retry_with_backoff_max_attempts() {
        let mut call_count = 0;
        let result = retry_with_backoff(
            || {
                call_count += 1;
                async { Err::<i32, &'static str>("persistent error") }
            },
            3,
            Duration::from_millis(10),
        ).await;
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "persistent error");
        assert_eq!(call_count, 3);
    }
    
    #[test]
    fn test_get_cached_regex_valid_pattern() {
        // Clear cache first
        {
            let mut cache = REGEX_CACHE.write().unwrap();
            cache.clear();
        }
        
        let pattern = r"[a-zA-Z]+";
        let result1 = get_cached_regex(pattern);
        assert!(result1.is_ok());
        
        let result2 = get_cached_regex(pattern);
        assert!(result2.is_ok());
        
        // Both should work and second should be from cache
        let regex1 = result1.unwrap();
        let regex2 = result2.unwrap();
        assert!(regex1.is_match("hello"));
        assert!(regex2.is_match("world"));
    }
    
    #[test]
    fn test_get_cached_regex_invalid_pattern() {
        let pattern = r"[";
        let result = get_cached_regex(pattern);
        assert!(result.is_err());
        
        if let Err(NCBError::InvalidRegex(msg)) = result {
            // The error message contains the pattern and the regex error
            assert!(msg.contains("["));
        } else {
            panic!("Expected InvalidRegex error");
        }
    }
    
    #[test]
    fn test_rate_limiter_basic() {
        let limiter = RateLimiter::new(5.0, 1.0); // 5 tokens, 1 per second
        
        // Should be able to acquire 5 tokens initially
        assert!(limiter.try_acquire(1.0));
        assert!(limiter.try_acquire(1.0));
        assert!(limiter.try_acquire(1.0));
        assert!(limiter.try_acquire(1.0));
        assert!(limiter.try_acquire(1.0));
        
        // 6th token should fail
        assert!(!limiter.try_acquire(1.0));
    }
    
    #[test]
    fn test_rate_limiter_partial_tokens() {
        let limiter = RateLimiter::new(2.0, 1.0);
        
        // Acquire partial tokens
        assert!(limiter.try_acquire(0.5));
        assert!(limiter.try_acquire(0.5));
        assert!(limiter.try_acquire(0.5));
        assert!(limiter.try_acquire(0.5));
        
        // Should fail with no tokens left
        assert!(!limiter.try_acquire(0.1));
    }
    
    #[test]
    fn test_performance_metrics_increment() {
        let metrics = PerformanceMetrics::default();
        
        assert_eq!(metrics.tts_requests.load(std::sync::atomic::Ordering::Relaxed), 0);
        
        metrics.increment_tts_requests();
        metrics.increment_tts_requests();
        
        assert_eq!(metrics.tts_requests.load(std::sync::atomic::Ordering::Relaxed), 2);
        
        metrics.increment_tts_cache_hits();
        assert_eq!(metrics.tts_cache_hits.load(std::sync::atomic::Ordering::Relaxed), 1);
        
        metrics.increment_tts_cache_misses();
        assert_eq!(metrics.tts_cache_misses.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
    
    #[test]
    fn test_metrics_snapshot_cache_hit_rate() {
        let snapshot = MetricsSnapshot {
            tts_requests: 10,
            tts_cache_hits: 7,
            tts_cache_misses: 3,
            regex_cache_hits: 0,
            regex_cache_misses: 0,
            database_operations: 0,
            voice_connections: 0,
        };
        
        assert!((snapshot.tts_cache_hit_rate() - 0.7).abs() < f64::EPSILON);
        
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
    }
    
    #[test]
    fn test_metrics_snapshot_regex_cache_hit_rate() {
        let snapshot = MetricsSnapshot {
            tts_requests: 0,
            tts_cache_hits: 0,
            tts_cache_misses: 0,
            regex_cache_hits: 8,
            regex_cache_misses: 2,
            database_operations: 0,
            voice_connections: 0,
        };
        
        assert!((snapshot.regex_cache_hit_rate() - 0.8).abs() < f64::EPSILON);
    }
    
    #[test]
    fn test_performance_metrics_get_stats() {
        let metrics = PerformanceMetrics::default();
        
        // Add some data
        metrics.increment_tts_requests();
        metrics.increment_tts_requests();
        metrics.increment_tts_cache_hits();
        metrics.increment_database_operations();
        
        let stats = metrics.get_stats();
        
        assert_eq!(stats.tts_requests, 2);
        assert_eq!(stats.tts_cache_hits, 1);
        assert_eq!(stats.tts_cache_misses, 0);
        assert_eq!(stats.database_operations, 1);
    }
}