//! CardKit 2.0 card primitives and builders.

mod builder;
mod validation;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value, json};

use crate::Result;

pub use builder::{CardBuilder, CardButtonStyle, CardElement};
use validation::validate_card_id;

const CARD_SCHEMA: &str = "2.0";

/// A validated Lark/Feishu CardKit 2.0 JSON card.
///
/// Use [`Card::builder`] for common components or [`Card::from_value`] when
/// working with official components not modeled by this crate yet. Validation
/// covers the shared-card, root/body, component-count, and identifier invariants
/// needed by this SDK; Lark/Feishu remains authoritative for component-specific
/// fields passed through raw JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct Card(Value);

impl Card {
    /// Starts a CardKit 2.0 builder.
    pub fn builder() -> CardBuilder {
        CardBuilder::new()
    }

    /// Creates a validated card from a complete CardKit JSON value.
    pub fn from_value(value: Value) -> Result<Self> {
        let card = Self(value);
        card.validate()?;
        Ok(card)
    }

    /// Wraps a raw CardKit body in a schema 2.0 card.
    ///
    pub fn raw(body: Value) -> Result<Self> {
        Self::from_value(json!({
            "schema": CARD_SCHEMA,
            "body": body,
        }))
    }

    /// Returns the complete CardKit JSON value.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// Consumes the card and returns the complete CardKit JSON value.
    pub fn into_value(self) -> Value {
        self.0
    }

    pub(crate) fn from_parts(
        config: Map<String, Value>,
        header: Option<Value>,
        elements: Vec<Value>,
    ) -> Result<Self> {
        let mut root = Map::new();
        root.insert("schema".to_owned(), Value::String(CARD_SCHEMA.to_owned()));
        root.insert("config".to_owned(), Value::Object(config));
        if let Some(header) = header {
            root.insert("header".to_owned(), header);
        }
        root.insert("body".to_owned(), json!({ "elements": elements }));
        Self::from_value(Value::Object(root))
    }
}

impl Serialize for Card {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Card {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_value(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// A CardKit card entity identifier returned by the create-card API.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CardId(pub String);

impl CardId {
    /// Creates and validates a CardKit card entity identifier.
    pub fn new(card_id: impl Into<String>) -> Result<Self> {
        let card_id = card_id.into();
        validate_card_id(&card_id)?;
        Ok(Self(card_id))
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_card_id(&self.0)
    }
}

impl<'de> Deserialize<'de> for CardId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}
