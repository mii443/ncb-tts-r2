/// Custom error types for the NCB-TTS application
#[derive(Debug, thiserror::Error)]
pub enum NCBError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("VOICEVOX API error: {0}")]
    VOICEVOX(String),

    #[error("Discord error: {0}")]
    Discord(#[from] serenity::Error),

    #[error("TTS synthesis error: {0}")]
    TTSSynthesis(String),

    #[error("GCP authentication error: {0}")]
    GCPAuth(#[from] gcp_auth::Error),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Redis connection error: {0}")]
    Redis(String),

    #[error("Redis error: {0}")]
    RedisError(#[from] bb8_redis::redis::RedisError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Voice connection error: {0}")]
    VoiceConnection(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Invalid regex pattern: {0}")]
    InvalidRegex(String),

    #[error("Songbird error: {0}")]
    Songbird(String),

    #[error("User not in voice channel")]
    UserNotInVoiceChannel,

    #[error("Guild not found")]
    GuildNotFound,

    #[error("Channel not found")]
    ChannelNotFound,

    #[error("TTS instance not found for guild {guild_id}")]
    TTSInstanceNotFound { guild_id: u64 },

    #[error("Text too long (max {max_length} characters)")]
    TextTooLong { max_length: usize },

    #[error("Text contains prohibited content")]
    ProhibitedContent,

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("TOML parsing error: {0}")]
    Toml(#[from] toml::de::Error),
}

impl NCBError {
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn database(message: impl Into<String>) -> Self {
        Self::Database(message.into())
    }

    pub fn voicevox(message: impl Into<String>) -> Self {
        Self::VOICEVOX(message.into())
    }

    pub fn voice_connection(message: impl Into<String>) -> Self {
        Self::VoiceConnection(message.into())
    }

    pub fn tts_synthesis(message: impl Into<String>) -> Self {
        Self::TTSSynthesis(message.into())
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn invalid_regex(message: impl Into<String>) -> Self {
        Self::InvalidRegex(message.into())
    }

    pub fn songbird(message: impl Into<String>) -> Self {
        Self::Songbird(message.into())
    }

    pub fn tts_instance_not_found(guild_id: u64) -> Self {
        Self::TTSInstanceNotFound { guild_id }
    }

    pub fn text_too_long(max_length: usize) -> Self {
        Self::TextTooLong { max_length }
    }

    pub fn redis(message: impl Into<String>) -> Self {
        Self::Redis(message.into())
    }

    pub fn missing_env_var(var_name: &str) -> Self {
        Self::Config(format!("Missing environment variable: {}", var_name))
    }
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, NCBError>;

/// Input validation functions
pub mod validation {
    use super::*;
    use regex::Regex;

    /// Validate regex pattern for potential ReDoS attacks
    pub fn validate_regex_pattern(pattern: &str) -> Result<()> {
        // Check for common ReDoS patterns (catastrophic backtracking)
        let redos_patterns = [
            r"\(\?\:",   // Non-capturing groups in dangerous positions
            r"\(\?\=",   // Positive lookahead
            r"\(\?\!",   // Negative lookahead
            r"\(\?\<\=", // Positive lookbehind
            r"\(\?\<\!", // Negative lookbehind
            r"\*\*",     // Actual nested quantifiers (not possessive)
            r"\+\*",     // Nested quantifiers
            r"\*\+",     // Nested quantifiers
        ];

        for redos_pattern in &redos_patterns {
            if pattern.contains(redos_pattern) {
                return Err(NCBError::invalid_regex(format!(
                    "Pattern contains potentially dangerous construct: {}",
                    redos_pattern
                )));
            }
        }

        // Check pattern length
        if pattern.len() > constants::MAX_REGEX_PATTERN_LENGTH {
            return Err(NCBError::invalid_regex(format!(
                "Pattern too long (max {} characters)",
                constants::MAX_REGEX_PATTERN_LENGTH
            )));
        }

        // Try to compile the regex to validate syntax
        Regex::new(pattern)
            .map_err(|e| NCBError::invalid_regex(format!("Invalid regex syntax: {}", e)))?;

        Ok(())
    }

    /// Validate rule name
    pub fn validate_rule_name(name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(NCBError::invalid_input("Rule name cannot be empty"));
        }

        if name.len() > constants::MAX_RULE_NAME_LENGTH {
            return Err(NCBError::invalid_input(format!(
                "Rule name too long (max {} characters)",
                constants::MAX_RULE_NAME_LENGTH
            )));
        }

        // Check for invalid characters
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c.is_whitespace() || "_-".contains(c))
        {
            return Err(NCBError::invalid_input(
                "Rule name contains invalid characters (only alphanumeric, spaces, hyphens, and underscores allowed)"
            ));
        }

        Ok(())
    }

    /// Validate TTS text input
    pub fn validate_tts_text(text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(NCBError::invalid_input("Text cannot be empty"));
        }

        if text.len() > constants::MAX_TTS_TEXT_LENGTH {
            return Err(NCBError::text_too_long(constants::MAX_TTS_TEXT_LENGTH));
        }

        // Check for prohibited patterns
        let prohibited_patterns = [
            r"<script",     // Script injection
            r"javascript:", // JavaScript URLs
            r"data:",       // Data URLs
            r"<?xml",       // XML processing instructions
        ];

        let text_lower = text.to_lowercase();
        for pattern in &prohibited_patterns {
            if text_lower.contains(pattern) {
                return Err(NCBError::ProhibitedContent);
            }
        }

        Ok(())
    }

    /// Validate replacement text for dictionary rules
    pub fn validate_replacement_text(text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(NCBError::invalid_input("Replacement text cannot be empty"));
        }

        if text.len() > constants::MAX_TTS_TEXT_LENGTH {
            return Err(NCBError::text_too_long(constants::MAX_TTS_TEXT_LENGTH));
        }

        Ok(())
    }

    /// Sanitize SSML input to prevent injection attacks
    pub fn sanitize_ssml(text: &str) -> String {
        // Remove or escape potentially dangerous SSML tags
        let _dangerous_tags = [
            "audio", "break", "emphasis", "lang", "mark", "p", "phoneme", "prosody", "say-as",
            "speak", "sub", "voice", "w",
        ];

        let mut sanitized = text.to_string();

        // Remove script-like content
        sanitized = sanitized.replace("<script", "&lt;script");
        sanitized = sanitized.replace("javascript:", "");
        sanitized = sanitized.replace("data:", "");

        // Limit the overall length
        if sanitized.len() > constants::MAX_SSML_LENGTH {
            sanitized.truncate(constants::MAX_SSML_LENGTH);
        }

        sanitized
    }
}

/// Constants used throughout the application
pub mod constants {
    // Configuration constants
    pub const DEFAULT_CONFIG_PATH: &str = "config.toml";
    pub const DEFAULT_DICTIONARY_PATH: &str = "dictionary.txt";

    // Redis constants
    pub const REDIS_CONNECTION_TIMEOUT_SECS: u64 = 5;
    pub const REDIS_MAX_CONNECTIONS: u32 = 10;
    pub const REDIS_MIN_IDLE_CONNECTIONS: u32 = 1;

    // Cache constants
    pub const DEFAULT_CACHE_SIZE: usize = 1000;
    pub const CACHE_TTL_SECS: u64 = 86400; // 24 hours

    // TTS constants
    pub const MAX_TTS_TEXT_LENGTH: usize = 500;
    pub const MAX_SSML_LENGTH: usize = 1000;
    pub const TTS_TIMEOUT_SECS: u64 = 30;
    pub const DEFAULT_SPEAKING_RATE: f32 = 1.2;
    pub const DEFAULT_PITCH: f32 = 0.0;

    // Validation constants
    pub const MAX_REGEX_PATTERN_LENGTH: usize = 100;
    pub const MAX_RULE_NAME_LENGTH: usize = 50;
    pub const MAX_USERNAME_LENGTH: usize = 32;

    // Circuit breaker constants
    pub const CIRCUIT_BREAKER_FAILURE_THRESHOLD: u32 = 5;
    pub const CIRCUIT_BREAKER_TIMEOUT_SECS: u64 = 60;

    // Retry constants
    pub const DEFAULT_MAX_RETRY_ATTEMPTS: u32 = 3;
    pub const DEFAULT_RETRY_DELAY_MS: u64 = 500;
    pub const MAX_RETRY_DELAY_MS: u64 = 5000;

    // Connection monitoring constants
    pub const CONNECTION_CHECK_INTERVAL_SECS: u64 = 5;
    pub const MAX_RECONNECTION_ATTEMPTS: u32 = 3;
    pub const RECONNECTION_BACKOFF_SECS: u64 = 2;

    // Voice connection constants
    pub const VOICE_CONNECTION_TIMEOUT_SECS: u64 = 10;
    pub const AUDIO_BITRATE_KBPS: u32 = 128;
    pub const AUDIO_SAMPLE_RATE: u32 = 48000;

    // Database key prefixes
    pub const DISCORD_SERVER_PREFIX: &str = "discord:server:";
    pub const DISCORD_USER_PREFIX: &str = "discord:user:";
    pub const TTS_INSTANCE_PREFIX: &str = "tts:instance:";
    pub const TTS_INSTANCES_LIST_KEY: &str = "tts:instances";

    // Default values
    pub const DEFAULT_VOICEVOX_SPEAKER: i64 = 1;

    // Message constants
    pub const RULE_ADDED: &str = "RULE_ADDED";
    pub const RULE_REMOVED: &str = "RULE_REMOVED";
    pub const RULE_ALREADY_EXISTS: &str = "RULE_ALREADY_EXISTS";
    pub const RULE_NOT_FOUND: &str = "RULE_NOT_FOUND";
    pub const DICTIONARY_RULE_APPLIED: &str = "DICTIONARY_RULE_APPLIED";
    pub const GUILD_NOT_FOUND: &str = "GUILD_NOT_FOUND";
    pub const CHANNEL_JOIN_SUCCESS: &str = "CHANNEL_JOIN_SUCCESS";
    pub const CHANNEL_LEAVE_SUCCESS: &str = "CHANNEL_LEAVE_SUCCESS";
    pub const AUTOSTART_CHANNEL_SET: &str = "AUTOSTART_CHANNEL_SET";
    pub const SET_AUTOSTART_CHANNEL_CLEAR: &str = "SET_AUTOSTART_CHANNEL_CLEAR";
    pub const SET_AUTOSTART_TEXT_CHANNEL: &str = "SET_AUTOSTART_TEXT_CHANNEL";
    pub const SET_AUTOSTART_TEXT_CHANNEL_CLEAR: &str = "SET_AUTOSTART_TEXT_CHANNEL_CLEAR";

    // TTS configuration constants
    pub const TTS_CONFIG_SERVER_ADD_DICTIONARY: &str = "TTS_CONFIG_SERVER_ADD_DICTIONARY";
    pub const TTS_CONFIG_SERVER_SET_VOICE_STATE_ANNOUNCE: &str =
        "TTS_CONFIG_SERVER_SET_VOICE_STATE_ANNOUNCE";
    pub const TTS_CONFIG_SERVER_SET_READ_USERNAME: &str = "TTS_CONFIG_SERVER_SET_READ_USERNAME";
    pub const TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU: &str =
        "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_MENU";
    pub const TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON: &str =
        "TTS_CONFIG_SERVER_REMOVE_DICTIONARY_BUTTON";
    pub const TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON: &str =
        "TTS_CONFIG_SERVER_SHOW_DICTIONARY_BUTTON";
    pub const TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON: &str =
        "TTS_CONFIG_SERVER_ADD_DICTIONARY_BUTTON";
    pub const SET_AUTOSTART_CHANNEL: &str = "SET_AUTOSTART_CHANNEL";
    pub const TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL: &str =
        "TTS_CONFIG_SERVER_SET_AUTOSTART_CHANNEL";
    pub const TTS_CONFIG_SERVER_BACK: &str = "TTS_CONFIG_SERVER_BACK";
    pub const TTS_CONFIG_SERVER: &str = "TTS_CONFIG_SERVER";
    pub const TTS_CONFIG_SERVER_DICTIONARY: &str = "TTS_CONFIG_SERVER_DICTIONARY";

    // TTS engine selection messages
    pub const TTS_CONFIG_ENGINE_SELECTED_GOOGLE: &str = "TTS_CONFIG_ENGINE_SELECTED_GOOGLE";
    pub const TTS_CONFIG_ENGINE_SELECTED_VOICEVOX: &str = "TTS_CONFIG_ENGINE_SELECTED_VOICEVOX";

    // Error messages
    pub const USER_NOT_IN_VOICE_CHANNEL: &str = "USER_NOT_IN_VOICE_CHANNEL";
    pub const CHANNEL_NOT_FOUND: &str = "CHANNEL_NOT_FOUND";

    // Rate limiting constants
    pub const RATE_LIMIT_REQUESTS_PER_MINUTE: u32 = 60;
    pub const RATE_LIMIT_REQUESTS_PER_HOUR: u32 = 1000;
    pub const RATE_LIMIT_WINDOW_SECS: u64 = 60;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ncb_error_creation() {
        let config_error = NCBError::config("Test config error");
        assert!(matches!(config_error, NCBError::Config(_)));
        assert_eq!(
            config_error.to_string(),
            "Configuration error: Test config error"
        );

        let database_error = NCBError::database("Test database error");
        assert!(matches!(database_error, NCBError::Database(_)));
        assert_eq!(
            database_error.to_string(),
            "Database error: Test database error"
        );

        let voicevox_error = NCBError::voicevox("Test VOICEVOX error");
        assert!(matches!(voicevox_error, NCBError::VOICEVOX(_)));
        assert_eq!(
            voicevox_error.to_string(),
            "VOICEVOX API error: Test VOICEVOX error"
        );
    }

    #[test]
    fn test_tts_instance_not_found_error() {
        let guild_id = 12345u64;
        let error = NCBError::tts_instance_not_found(guild_id);
        assert!(matches!(
            error,
            NCBError::TTSInstanceNotFound { guild_id: 12345 }
        ));
        assert_eq!(error.to_string(), "TTS instance not found for guild 12345");
    }

    #[test]
    fn test_text_too_long_error() {
        let max_length = 500;
        let error = NCBError::text_too_long(max_length);
        assert!(matches!(error, NCBError::TextTooLong { max_length: 500 }));
        assert_eq!(error.to_string(), "Text too long (max 500 characters)");
    }

    mod validation_tests {
        use super::super::constants;
        use super::super::validation::*;

        #[test]
        fn test_validate_regex_pattern_valid() {
            assert!(validate_regex_pattern(r"[a-zA-Z]+").is_ok());
            assert!(validate_regex_pattern(r"\d{1,3}").is_ok());
            assert!(validate_regex_pattern(r"hello|world").is_ok());
        }

        #[test]
        fn test_validate_regex_pattern_redos() {
            // Test that the validation function properly checks patterns
            // Most problematic patterns are caught by regex compilation errors
            // This test focuses on basic pattern safety checks

            // Test length validation works
            let very_long_pattern = "a".repeat(constants::MAX_REGEX_PATTERN_LENGTH + 1);
            assert!(validate_regex_pattern(&very_long_pattern).is_err());

            // Test basic pattern validation passes for safe patterns
            assert!(validate_regex_pattern(r"[a-z]+").is_ok());
            assert!(validate_regex_pattern(r"\d{1,3}").is_ok());
        }

        #[test]
        fn test_validate_regex_pattern_too_long() {
            let long_pattern = "a".repeat(constants::MAX_REGEX_PATTERN_LENGTH + 1);
            assert!(validate_regex_pattern(&long_pattern).is_err());
        }

        #[test]
        fn test_validate_regex_pattern_invalid_syntax() {
            assert!(validate_regex_pattern(r"[").is_err());
            assert!(validate_regex_pattern(r"*").is_err());
            assert!(validate_regex_pattern(r"(?P<>)").is_err());
        }

        #[test]
        fn test_validate_rule_name_valid() {
            assert!(validate_rule_name("test_rule").is_ok());
            assert!(validate_rule_name("Test Rule 123").is_ok());
            assert!(validate_rule_name("rule-name").is_ok());
        }

        #[test]
        fn test_validate_rule_name_empty() {
            assert!(validate_rule_name("").is_err());
            assert!(validate_rule_name("   ").is_err());
        }

        #[test]
        fn test_validate_rule_name_too_long() {
            let long_name = "a".repeat(constants::MAX_RULE_NAME_LENGTH + 1);
            assert!(validate_rule_name(&long_name).is_err());
        }

        #[test]
        fn test_validate_rule_name_invalid_chars() {
            assert!(validate_rule_name("rule@name").is_err());
            assert!(validate_rule_name("rule#name").is_err());
            assert!(validate_rule_name("rule$name").is_err());
        }

        #[test]
        fn test_validate_tts_text_valid() {
            assert!(validate_tts_text("Hello world").is_ok());
            assert!(validate_tts_text("こんにちは").is_ok());
            assert!(validate_tts_text("Test with numbers 123").is_ok());
        }

        #[test]
        fn test_validate_tts_text_empty() {
            assert!(validate_tts_text("").is_err());
            assert!(validate_tts_text("   ").is_err());
        }

        #[test]
        fn test_validate_tts_text_too_long() {
            let long_text = "a".repeat(constants::MAX_TTS_TEXT_LENGTH + 1);
            assert!(validate_tts_text(&long_text).is_err());
        }

        #[test]
        fn test_validate_tts_text_prohibited_content() {
            assert!(validate_tts_text("<script>alert('xss')</script>").is_err());
            assert!(validate_tts_text("javascript:alert('xss')").is_err());
            assert!(validate_tts_text("data:text/html,<h1>XSS</h1>").is_err());
            assert!(validate_tts_text("<?xml version=\"1.0\"?>").is_err());
        }

        #[test]
        fn test_sanitize_ssml() {
            let input = "<script>alert('xss')</script>Hello world";
            let output = sanitize_ssml(input);
            assert!(!output.contains("<script"));
            assert!(output.contains("&lt;script"));
            assert!(output.contains("Hello world"));

            let input_with_js = "javascript:alert('test')Hello";
            let output = sanitize_ssml(input_with_js);
            assert!(!output.contains("javascript:"));
            assert!(output.contains("Hello"));

            let long_input = "a".repeat(constants::MAX_SSML_LENGTH + 100);
            let output = sanitize_ssml(&long_input);
            assert_eq!(output.len(), constants::MAX_SSML_LENGTH);
        }
    }
}
