// Public API for the NCB-TTS-R2 library

pub mod errors;
pub mod utils;
pub mod tts;
pub mod database;
pub mod config;
pub mod data;
pub mod implement;
pub mod events;
pub mod commands;
pub mod stream_input;
pub mod trace;
pub mod event_handler;
pub mod connection_monitor;

// Re-export commonly used types
pub use errors::{NCBError, Result};
pub use utils::{CircuitBreaker, CircuitBreakerState, retry_with_backoff, get_cached_regex, PerformanceMetrics};
pub use tts::tts_type::TTSType;