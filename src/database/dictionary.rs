use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub is_regex: bool,
    pub rule: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dictionary {
    pub rules: Vec<Rule>,
}

impl Dictionary {
    pub fn new() -> Self {
        let rules = vec![
            Rule {
                id: String::from("url"),
                is_regex: true,
                rule: String::from(r"(http://|https://){1}[\w\.\-/:\#\?=\&;%\~\+]+"),
                to: String::from("URL"),
            },
            Rule {
                id: String::from("code"),
                is_regex: true,
                rule: String::from(r"```(.|\n)*```"),
                to: String::from("code"),
            },
        ];
        Self { rules }
    }
}
