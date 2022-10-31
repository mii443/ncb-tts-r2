use serde::{Deserialize, Serialize};

use super::mora::Mora;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AccentPhrase {
    pub moras: Vec<Mora>,
    pub accent: f64,
    pub pause_mora: Option<Mora>,
    pub is_interrogative: bool,
}
