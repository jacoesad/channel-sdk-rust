use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub schema: String,
    pub body: Value,
}

impl Card {
    pub fn raw(body: Value) -> Self {
        Self {
            schema: "2.0".to_owned(),
            body,
        }
    }
}
