use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct SynthesizeResponse {
    pub audioContent: String
}