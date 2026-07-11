//! CardKit 2.0 card primitives and builders.

mod builder;

use std::collections::HashSet;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value, json};

use crate::validation::validate_path_identifier;
use crate::{Error, Result};

pub use builder::{CardBuilder, CardButtonStyle, CardElement};

const CARD_SCHEMA: &str = "2.0";
const MAX_CARD_ID_CHARS: usize = 20;
const MAX_CARD_COMPONENTS: usize = 200;
const MAX_ELEMENT_ID_CHARS: usize = 20;
const OPAQUE_CARD_FIELDS: [&str; 6] = [
    "chart_spec",
    "data",
    "rows",
    "template_variable",
    "value",
    "variables",
];

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

    pub(crate) fn validate(&self) -> Result<()> {
        let root = self
            .0
            .as_object()
            .ok_or_else(|| Error::Validation("card must be a JSON object".to_owned()))?;

        if root.get("schema").and_then(Value::as_str) != Some(CARD_SCHEMA) {
            return Err(Error::Validation("card schema must be \"2.0\"".to_owned()));
        }

        if let Some(config) = root.get("config") {
            let config = config
                .as_object()
                .ok_or_else(|| Error::Validation("card config must be a JSON object".to_owned()))?;
            if config
                .get("update_multi")
                .is_some_and(|value| value != &Value::Bool(true))
            {
                return Err(Error::Validation(
                    "CardKit 2.0 config.update_multi must be true when provided".to_owned(),
                ));
            }
        }

        let body = root
            .get("body")
            .ok_or_else(|| Error::Validation("card body must be a JSON object".to_owned()))?;
        let body_object = body
            .as_object()
            .ok_or_else(|| Error::Validation("card body must be a JSON object".to_owned()))?;
        let elements = body_object
            .get("elements")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                Error::Validation("card body.elements must be a JSON array".to_owned())
            })?;

        if let Some(header) = root.get("header") {
            validate_header(header)?;
        }

        if root.get("header").is_none() && elements.is_empty() {
            return Err(Error::Validation(
                "card must contain a header or at least one body element".to_owned(),
            ));
        }

        for element in elements {
            let element = element.as_object().ok_or_else(|| {
                Error::Validation("card body elements must be JSON objects".to_owned())
            })?;
            if element.get("tag").and_then(Value::as_str).is_none() {
                return Err(Error::Validation(
                    "card body elements must contain a string tag".to_owned(),
                ));
            }
        }

        let mut component_count = 0;
        let mut element_ids = HashSet::new();
        if let Some(header) = root.get("header") {
            validate_components(header, &mut component_count, &mut element_ids)?;
        }
        validate_components(body, &mut component_count, &mut element_ids)
    }

    pub(crate) fn validate_for_message_update(&self) -> Result<()> {
        self.validate()?;
        let update_multi = self
            .0
            .get("config")
            .and_then(Value::as_object)
            .and_then(|config| config.get("update_multi"))
            .and_then(Value::as_bool);
        if update_multi != Some(true) {
            return Err(Error::Validation(
                "message card updates require config.update_multi=true".to_owned(),
            ));
        }
        Ok(())
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

pub(crate) fn validate_element_id(element_id: &str) -> Result<()> {
    let mut chars = element_id.chars();
    let Some(first) = chars.next() else {
        return Err(Error::Validation(
            "card element_id must not be empty".to_owned(),
        ));
    };
    if !first.is_ascii_alphabetic()
        || !chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(Error::Validation(
            "card element_id must start with a letter and contain only ASCII letters, digits, or underscores"
                .to_owned(),
        ));
    }
    if element_id.chars().count() > MAX_ELEMENT_ID_CHARS {
        return Err(Error::Validation(format!(
            "card element_id must be at most {MAX_ELEMENT_ID_CHARS} characters"
        )));
    }
    Ok(())
}

fn validate_card_id(card_id: &str) -> Result<()> {
    if card_id.is_empty() {
        return Err(Error::Validation("card_id must not be empty".to_owned()));
    }
    if card_id.chars().count() > MAX_CARD_ID_CHARS {
        return Err(Error::Validation(format!(
            "card_id must be at most {MAX_CARD_ID_CHARS} characters"
        )));
    }
    validate_path_identifier(card_id, "card_id")
}

fn validate_header(header: &Value) -> Result<()> {
    let header = header
        .as_object()
        .ok_or_else(|| Error::Validation("card header must be a JSON object".to_owned()))?;
    let title = header
        .get("title")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Validation("card header.title must be a JSON object".to_owned()))?;
    if !matches!(
        title.get("tag").and_then(Value::as_str),
        Some("plain_text" | "lark_md")
    ) {
        return Err(Error::Validation(
            "card header.title tag must be plain_text or lark_md".to_owned(),
        ));
    }
    let has_content = match title.get("content") {
        Some(Value::String(_)) => true,
        Some(_) => {
            return Err(Error::Validation(
                "card header.title content must be a string".to_owned(),
            ));
        }
        None => false,
    };
    let mut has_localized_content = false;
    for field in ["i18n_content", "i18n"] {
        let Some(translations) = title.get(field) else {
            continue;
        };
        let translations = translations.as_object().ok_or_else(|| {
            Error::Validation(format!("card header.title {field} must be a JSON object"))
        })?;
        if translations.is_empty() || !translations.values().all(Value::is_string) {
            return Err(Error::Validation(format!(
                "card header.title {field} must contain at least one string translation"
            )));
        }
        has_localized_content = true;
    }
    if !has_content && !has_localized_content {
        return Err(Error::Validation(
            "card header.title must contain string content or localized string content".to_owned(),
        ));
    }
    Ok(())
}

fn validate_components(
    value: &Value,
    component_count: &mut usize,
    seen: &mut HashSet<String>,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            if object.get("tag").and_then(Value::as_str).is_some() {
                *component_count += 1;
                if *component_count > MAX_CARD_COMPONENTS {
                    return Err(Error::Validation(format!(
                        "card must contain at most {MAX_CARD_COMPONENTS} components and elements"
                    )));
                }
                if let Some(element_id) = object.get("element_id") {
                    let element_id = element_id.as_str().ok_or_else(|| {
                        Error::Validation("card element_id must be a string".to_owned())
                    })?;
                    validate_element_id(element_id)?;
                    if !seen.insert(element_id.to_owned()) {
                        return Err(Error::Validation(format!(
                            "card element_id must be unique: {element_id}"
                        )));
                    }
                }
            }
            for (key, nested) in object {
                if !OPAQUE_CARD_FIELDS.contains(&key.as_str()) {
                    validate_components(nested, component_count, seen)?;
                }
            }
        }
        Value::Array(values) => {
            for nested in values {
                validate_components(nested, component_count, seen)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn deserialization_rejects_duplicate_element_ids() {
        let error = serde_json::from_value::<Card>(json!({
            "schema": "2.0",
            "body": {
                "elements": [
                    { "tag": "markdown", "content": "one", "element_id": "same_id" },
                    { "tag": "hr", "element_id": "same_id" }
                ]
            }
        }))
        .expect_err("duplicate element ids must fail");

        assert!(error.to_string().contains("element_id must be unique"));
    }

    #[test]
    fn message_update_requires_explicit_shared_card_config() {
        let card = Card::raw(json!({
            "elements": [{ "tag": "markdown", "content": "hello" }]
        }))
        .expect("valid card body");

        let error = card
            .validate_for_message_update()
            .expect_err("missing update_multi must fail for message patch");

        assert!(matches!(
            error,
            Error::Validation(message)
                if message == "message card updates require config.update_multi=true"
        ));
    }

    #[test]
    fn callback_payload_element_id_is_not_treated_as_a_component_id() {
        let card = Card::from_value(json!({
            "schema": "2.0",
            "body": {
                "elements": [{
                    "tag": "button",
                    "element_id": "button_1",
                    "text": { "tag": "plain_text", "content": "Approve" },
                    "behaviors": [{
                        "type": "callback",
                        "value": {
                            "tag": "business_event",
                            "element_id": "business-task/42"
                        }
                    }]
                }]
            }
        }))
        .expect("callback payload fields are opaque business data");

        assert_eq!(
            card.as_value()["body"]["elements"][0]["behaviors"][0]["value"]["element_id"],
            "business-task/42"
        );
    }

    #[test]
    fn chart_spec_data_is_not_treated_as_card_components() {
        Card::from_value(json!({
            "schema": "2.0",
            "body": {
                "elements": [{
                    "tag": "chart",
                    "chart_spec": {
                        "type": "bar",
                        "data": {
                            "values": [{
                                "tag": "business_dimension",
                                "element_id": "not/a/component"
                            }]
                        }
                    }
                }]
            }
        }))
        .expect("chart specification is opaque business data");
    }

    #[test]
    fn enforces_cardkit_component_limit() {
        let mut builder = Card::builder();
        for _ in 0..MAX_CARD_COMPONENTS {
            builder = builder.markdown("component");
        }
        builder.build().expect("two hundred components are valid");

        let mut builder = Card::builder();
        for _ in 0..=MAX_CARD_COMPONENTS {
            builder = builder.markdown("component");
        }
        let error = builder
            .build()
            .expect_err("two hundred and one components must fail");
        assert!(matches!(
            error,
            Error::Validation(message)
                if message == "card must contain at most 200 components and elements"
        ));
    }

    #[test]
    fn card_id_rejects_url_path_delimiters() {
        for card_id in ["card/1", "card?x=1", "card#fragment", "card%2F1"] {
            let error = CardId::new(card_id).expect_err("unsafe card_id must fail");
            assert!(matches!(error, Error::Validation(_)));
        }
    }

    #[test]
    fn accepts_localized_header_title_without_default_content() {
        for localized_field in ["i18n_content", "i18n"] {
            let card = Card::from_value(json!({
                "schema": "2.0",
                "header": {
                    "title": {
                        "tag": "plain_text",
                        (localized_field): {
                            "zh_cn": "部署状态",
                            "en_us": "Deployment status"
                        }
                    }
                },
                "body": { "elements": [] }
            }))
            .expect("localized title is valid");

            assert_eq!(
                card.as_value()["header"]["title"][localized_field]["en_us"],
                "Deployment status"
            );
        }
    }

    #[test]
    fn rejects_each_malformed_header_title_representation() {
        let invalid_titles = [
            json!({
                "tag": "plain_text",
                "content": 42,
                "i18n_content": { "en_us": "Deployment status" }
            }),
            json!({
                "tag": "plain_text",
                "content": "Deployment status",
                "i18n_content": { "en_us": 42 }
            }),
            json!({
                "tag": "plain_text",
                "content": "Deployment status",
                "i18n": "not-an-object"
            }),
        ];

        for title in invalid_titles {
            let error = Card::from_value(json!({
                "schema": "2.0",
                "header": { "title": title },
                "body": { "elements": [] }
            }))
            .expect_err("each present title representation must be valid");
            assert!(matches!(error, Error::Validation(_)));
        }
    }
}
