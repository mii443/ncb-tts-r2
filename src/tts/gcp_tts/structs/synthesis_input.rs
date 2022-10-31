use serde::{Deserialize, Serialize};

/// Example:
/// ```rust
/// SynthesisInput {
///     text: None,
///     ssml: Some(String::from("<speak>test</speak>"))
/// }
/// ```
#[derive(Serialize, Deserialize, Debug)]
pub struct SynthesisInput {
    pub text: Option<String>,
    pub ssml: Option<String>,
}
