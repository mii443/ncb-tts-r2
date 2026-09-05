use crate::errors::{NCBError, Result};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub prefix: String,
    pub token: String,
    pub application_id: u64,
    pub redis_url: String,
    pub voicevox_key: Option<String>,
    pub voicevox_original_api_url: Option<String>,
    pub otel_http_url: Option<String>,
    #[serde(default)]
    pub transcription: TranscriptionConfig,
    #[serde(default)]
    pub web: WebConfig,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    pub enabled: bool,
    pub url: String,
    pub secret: String,
    pub require_consent: bool,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            enabled: cfg!(feature = "transcription"),
            url: "ws://127.0.0.1:8766/ingest/v2".into(),
            secret: String::new(),
            require_consent: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub enabled: bool,
    pub bind: String,
    pub base_url: String,
    pub client_id: String,
    pub client_secret: String,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: cfg!(feature = "web-ui"),
            bind: "127.0.0.1:8080".into(),
            base_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
        }
    }
}

impl Config {
    /// Feature environment variables override TOML, including when using a config file.
    pub fn configure_features(&mut self) -> Result<()> {
        self.apply_feature_env(|name| std::env::var(name).ok())?;
        self.validate_features()
    }

    fn apply_feature_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Result<()> {
        if let Some(value) = get("NCB_TRANSCRIPTION_ENABLED") {
            self.transcription.enabled = parse_bool("NCB_TRANSCRIPTION_ENABLED", &value)?;
        }
        if let Some(value) = get("NCB_WEB_ENABLED") {
            self.web.enabled = parse_bool("NCB_WEB_ENABLED", &value)?;
        }
        // Disabling transcription also disables its Web UI. Disabled features do
        // not read or validate their credentials, URLs, or other settings.
        if !self.transcription.enabled {
            self.web.enabled = false;
            return Ok(());
        }
        if let Some(value) = get("HAYAMIMI_URL") {
            self.transcription.url = value;
        }
        if let Some(value) = get("HAYAMIMI_BRIDGE_SECRET") {
            self.transcription.secret = value;
        }
        if let Some(value) = get("NCB_REQUIRE_CONSENT").or_else(|| get("RSTT_REQUIRE_CONSENT")) {
            self.transcription.require_consent = parse_bool("NCB_REQUIRE_CONSENT", &value)?;
        }
        if self.web.enabled {
            for (name, legacy, field) in [
                ("NCB_WEB_BIND", "RSTT_WEB_BIND", &mut self.web.bind),
                (
                    "NCB_WEB_BASE_URL",
                    "RSTT_WEB_BASE_URL",
                    &mut self.web.base_url,
                ),
                (
                    "NCB_WEB_CLIENT_ID",
                    "DISCORD_CLIENT_ID",
                    &mut self.web.client_id,
                ),
                (
                    "NCB_WEB_CLIENT_SECRET",
                    "DISCORD_CLIENT_SECRET",
                    &mut self.web.client_secret,
                ),
            ] {
                if let Some(value) = get(name).or_else(|| get(legacy)) {
                    *field = value;
                }
            }
            if self.web.client_id.is_empty() {
                self.web.client_id = self.application_id.to_string();
            }
        }
        Ok(())
    }

    fn validate_features(&self) -> Result<()> {
        if !self.transcription.enabled {
            return Ok(());
        }
        if !cfg!(feature = "transcription") {
            return Err(NCBError::config(
                "transcription was not compiled into this binary",
            ));
        }
        let url = reqwest::Url::parse(&self.transcription.url)
            .map_err(|_| NCBError::config("HAYAMIMI_URL must be a valid ws:// or wss:// URL"))?;
        if !matches!(url.scheme(), "ws" | "wss")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(NCBError::config(
                "HAYAMIMI_URL must be a ws:// or wss:// URL without credentials or fragment",
            ));
        }
        if self.transcription.secret.trim().is_empty() {
            return Err(NCBError::config(
                "HAYAMIMI_BRIDGE_SECRET is required when transcription is enabled",
            ));
        }
        if self.web.enabled && !cfg!(feature = "web-ui") {
            return Err(NCBError::config("web-ui was not compiled into this binary"));
        }
        #[cfg(feature = "web-ui")]
        if self.web.enabled {
            crate::transcription::web::WebConfig::new(&self.web)
                .map_err(|e| NCBError::config(e.to_string()))?;
        }
        Ok(())
    }
}

fn parse_bool(name: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(NCBError::config(format!(
            "{name} must be true/false, 1/0, yes/no, or on/off"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        toml::from_str(
            "prefix = '!'\ntoken = 'test'\napplication_id = 42\nredis_url = 'redis://localhost'",
        )
        .unwrap()
    }
    #[test]
    fn defaults_follow_compiled_features() {
        let c = config();
        assert_eq!(c.transcription.enabled, cfg!(feature = "transcription"));
        assert_eq!(c.web.enabled, cfg!(feature = "web-ui"));
    }
    #[test]
    fn disabled_features_need_no_secrets_or_valid_endpoints() {
        let mut c = config();
        c.transcription.url = "invalid".into();
        c.web.base_url = "invalid".into();
        c.apply_feature_env(|name| match name {
            "NCB_TRANSCRIPTION_ENABLED" => Some("false".into()),
            "RSTT_REQUIRE_CONSENT" => Some("invalid".into()),
            _ => None,
        })
        .unwrap();
        c.validate_features().unwrap();
        assert!(!c.web.enabled);
    }
    #[test]
    fn environment_overrides_toml_and_accepts_rstt_settings() {
        let mut c = config();
        c.apply_feature_env(|name| match name {
            "NCB_TRANSCRIPTION_ENABLED" | "NCB_WEB_ENABLED" => Some("true".into()),
            "HAYAMIMI_BRIDGE_SECRET" => Some("bridge".into()),
            "RSTT_WEB_BASE_URL" => Some("https://rstt.example.com".into()),
            "DISCORD_CLIENT_SECRET" => Some("oauth".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(c.web.client_id, "42");
        assert_eq!(c.web.base_url, "https://rstt.example.com");
        assert_eq!(c.validate_features().is_ok(), cfg!(feature = "web-ui"));
    }
    #[test]
    fn web_can_be_disabled_independently() {
        let mut c = config();
        c.apply_feature_env(|name| match name {
            "NCB_WEB_ENABLED" => Some("off".into()),
            "HAYAMIMI_BRIDGE_SECRET" => Some("bridge".into()),
            _ => None,
        })
        .unwrap();
        c.validate_features().unwrap();
        assert!(!c.web.enabled);
    }
    #[test]
    fn invalid_switch_is_rejected_without_echoing_value() {
        assert!(parse_bool("NCB_WEB_ENABLED", "private-value")
            .unwrap_err()
            .to_string()
            .contains("must be"));
        assert!(!parse_bool("NCB_WEB_ENABLED", "private-value")
            .unwrap_err()
            .to_string()
            .contains("private-value"));
    }
}
