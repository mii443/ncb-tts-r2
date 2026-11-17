// Public API for the NCB-TTS-R2 library

pub mod commands;
pub mod config;
pub mod connection_monitor;
pub mod data;
pub mod database;
pub mod errors;
pub mod event_handler;
pub mod events;
pub mod implement;
pub mod interactions;
pub mod stream_input;
pub mod trace;
pub mod tts;
pub mod utils;

// Re-export commonly used types
pub use errors::{NCBError, Result};
pub use tts::tts_type::TTSType;
pub use utils::{
    get_cached_regex, retry_with_backoff, CircuitBreaker, CircuitBreakerState, PerformanceMetrics,
};

#[cfg(feature = "transcription")]
pub mod transcription;
