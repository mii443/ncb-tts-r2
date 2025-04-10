use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub prefix: String,
    pub token: String,
    pub application_id: u64,
    pub redis_url: String,
    pub voicevox_key: String,
    pub otel_http_url: Option<String>,
}
