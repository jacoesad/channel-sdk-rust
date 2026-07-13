use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

use crate::{Error, Result};

/// Maximum full-text length accepted by the CardKit streaming content API.
pub const MAX_CARD_ELEMENT_CONTENT_CHARS: usize = 100_000;

/// Validated full text for a CardKit streaming element update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardElementContent(String);

impl CardElementContent {
    pub fn new(content: impl Into<String>) -> Result<Self> {
        let content = content.into();
        validate_card_element_content(&content)?;
        Ok(Self(content))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

pub(crate) fn validate_card_element_content(content: &str) -> Result<()> {
    let content_chars = content.chars().count();
    if content_chars == 0 || content_chars > MAX_CARD_ELEMENT_CONTENT_CHARS {
        return Err(Error::Validation(format!(
            "card element content must be between 1 and {MAX_CARD_ELEMENT_CONTENT_CHARS} characters"
        )));
    }
    Ok(())
}

/// Per-client numeric values used by CardKit streaming text configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardStreamingPlatformValues {
    pub default: u32,
    pub android: Option<u32>,
    pub ios: Option<u32>,
    pub pc: Option<u32>,
}

impl CardStreamingPlatformValues {
    /// Creates values with one default shared by every client.
    pub fn new(default: u32) -> Self {
        Self {
            default,
            android: None,
            ios: None,
            pc: None,
        }
    }

    pub fn android(mut self, value: u32) -> Self {
        self.android = Some(value);
        self
    }

    pub fn ios(mut self, value: u32) -> Self {
        self.ios = Some(value);
        self
    }

    pub fn pc(mut self, value: u32) -> Self {
        self.pc = Some(value);
        self
    }
}

impl From<CardStreamingPlatformValues> for Value {
    fn from(values: CardStreamingPlatformValues) -> Self {
        let mut object = Map::new();
        object.insert("default".to_owned(), Value::from(values.default));
        if let Some(value) = values.android {
            object.insert("android".to_owned(), Value::from(value));
        }
        if let Some(value) = values.ios {
            object.insert("ios".to_owned(), Value::from(value));
        }
        if let Some(value) = values.pc {
            object.insert("pc".to_owned(), Value::from(value));
        }
        Self::Object(object)
    }
}

impl Serialize for CardStreamingPlatformValues {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Value::from(self.clone()).serialize(serializer)
    }
}

/// CardKit client strategy for text that has not finished rendering yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CardStreamingPrintStrategy {
    #[default]
    Fast,
    Delay,
}

impl Serialize for CardStreamingPrintStrategy {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl CardStreamingPrintStrategy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Delay => "delay",
        }
    }
}

/// CardKit typewriter rendering configuration.
///
/// The default matches the official streaming-card guide: 70 milliseconds,
/// one character per step, and the `fast` print strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardStreamingConfig {
    pub print_frequency_ms: CardStreamingPlatformValues,
    pub print_step: CardStreamingPlatformValues,
    pub print_strategy: CardStreamingPrintStrategy,
}

impl Default for CardStreamingConfig {
    fn default() -> Self {
        Self {
            print_frequency_ms: CardStreamingPlatformValues::new(70),
            print_step: CardStreamingPlatformValues::new(1),
            print_strategy: CardStreamingPrintStrategy::Fast,
        }
    }
}

impl CardStreamingConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn print_frequency_ms(mut self, values: CardStreamingPlatformValues) -> Self {
        self.print_frequency_ms = values;
        self
    }

    pub fn print_step(mut self, values: CardStreamingPlatformValues) -> Self {
        self.print_step = values;
        self
    }

    pub fn print_strategy(mut self, strategy: CardStreamingPrintStrategy) -> Self {
        self.print_strategy = strategy;
        self
    }
}

impl From<CardStreamingConfig> for Value {
    fn from(config: CardStreamingConfig) -> Self {
        let mut object = Map::new();
        object.insert(
            "print_frequency_ms".to_owned(),
            Value::from(config.print_frequency_ms),
        );
        object.insert("print_step".to_owned(), Value::from(config.print_step));
        object.insert(
            "print_strategy".to_owned(),
            Value::String(config.print_strategy.as_str().to_owned()),
        );
        Self::Object(object)
    }
}

impl Serialize for CardStreamingConfig {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Value::from(self.clone()).serialize(serializer)
    }
}

/// CardKit entity settings supported by the streaming workflow.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CardSettings {
    config: CardSettingsConfig,
}

impl CardSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables CardKit streaming mode.
    pub fn streaming_mode(mut self, enabled: bool) -> Self {
        self.config.streaming_mode = Some(enabled);
        self
    }

    /// Updates the CardKit typewriter rendering configuration.
    pub fn streaming_config(mut self, config: CardStreamingConfig) -> Self {
        self.config.streaming_config = Some(config);
        self
    }

    /// Updates the card summary shown in chat previews.
    pub fn summary(mut self, content: impl Into<String>) -> Self {
        self.config.summary = Some(CardSummary {
            content: content.into(),
        });
        self
    }

    pub fn is_empty(&self) -> bool {
        self.config.streaming_mode.is_none()
            && self.config.streaming_config.is_none()
            && self.config.summary.is_none()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.is_empty() {
            return Err(Error::Validation(
                "card settings must contain at least one update".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
struct CardSettingsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    streaming_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    streaming_config: Option<CardStreamingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<CardSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CardSummary {
    content: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_streaming_content_by_unicode_characters() {
        let content = CardElementContent::new("界".repeat(MAX_CARD_ELEMENT_CONTENT_CHARS))
            .expect("exact content limit");
        assert_eq!(
            content.as_str().chars().count(),
            MAX_CARD_ELEMENT_CONTENT_CHARS
        );

        for invalid in [
            String::new(),
            "界".repeat(MAX_CARD_ELEMENT_CONTENT_CHARS + 1),
        ] {
            assert!(CardElementContent::new(invalid).is_err());
        }
    }

    #[test]
    fn serializes_default_streaming_config() {
        assert_eq!(
            serde_json::to_value(CardStreamingConfig::new()).expect("streaming config"),
            json!({
                "print_frequency_ms": { "default": 70 },
                "print_step": { "default": 1 },
                "print_strategy": "fast"
            })
        );
    }

    #[test]
    fn serializes_platform_overrides_and_delayed_strategy() {
        let config = CardStreamingConfig::new()
            .print_frequency_ms(
                CardStreamingPlatformValues::new(80)
                    .android(90)
                    .ios(85)
                    .pc(60),
            )
            .print_step(CardStreamingPlatformValues::new(2).pc(3))
            .print_strategy(CardStreamingPrintStrategy::Delay);

        assert_eq!(
            serde_json::to_value(config).expect("streaming config"),
            json!({
                "print_frequency_ms": {
                    "default": 80,
                    "android": 90,
                    "ios": 85,
                    "pc": 60
                },
                "print_step": { "default": 2, "pc": 3 },
                "print_strategy": "delay"
            })
        );
    }

    #[test]
    fn serializes_streaming_settings_under_config() {
        let settings = CardSettings::new()
            .streaming_mode(true)
            .streaming_config(CardStreamingConfig::new())
            .summary("Finished");

        assert_eq!(
            serde_json::to_value(settings).expect("settings"),
            json!({
                "config": {
                    "streaming_mode": true,
                    "streaming_config": {
                        "print_frequency_ms": { "default": 70 },
                        "print_step": { "default": 1 },
                        "print_strategy": "fast"
                    },
                    "summary": { "content": "Finished" }
                }
            })
        );
    }

    #[test]
    fn rejects_empty_settings() {
        let error = CardSettings::new()
            .validate()
            .expect_err("empty settings must fail");

        assert!(matches!(error, Error::Validation(_)));
    }
}
