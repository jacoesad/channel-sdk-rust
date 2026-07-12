use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value, json};
use url::Url;

use crate::{Error, Result};

use super::Card;
use super::validation::validate_element_id;

/// Supported visual styles for a CardKit button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CardButtonStyle {
    #[default]
    Default,
    Primary,
    Danger,
}

impl CardButtonStyle {
    fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Primary => "primary",
            Self::Danger => "danger",
        }
    }
}

/// A CardKit 2.0 body element.
///
/// Constructors cover common Channel workflows. [`CardElement::raw`] is an
/// escape hatch for official CardKit components not modeled here yet.
#[derive(Debug, Clone, PartialEq)]
pub struct CardElement(Value);

impl CardElement {
    /// Creates a native CardKit Markdown component.
    pub fn markdown(content: impl Into<String>) -> Self {
        Self(json!({ "tag": "markdown", "content": content.into() }))
    }

    /// Creates a plain-text `div` component.
    pub fn text(content: impl Into<String>) -> Self {
        Self(json!({
            "tag": "div",
            "text": { "tag": "plain_text", "content": content.into() }
        }))
    }

    /// Creates a divider component.
    pub fn divider() -> Self {
        Self(json!({ "tag": "hr" }))
    }

    /// Creates a button that emits a `card.action.trigger` callback.
    pub fn callback_button(label: impl Into<String>, value: Value) -> Result<Self> {
        if !value.is_object() {
            return Err(Error::Validation(
                "card callback button value must be a JSON object".to_owned(),
            ));
        }
        Ok(Self(json!({
            "tag": "button",
            "text": { "tag": "plain_text", "content": label.into() },
            "type": "default",
            "behaviors": [{ "type": "callback", "value": value }]
        })))
    }

    /// Creates a button that opens the same URL on all supported clients.
    pub fn open_url_button(label: impl Into<String>, url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        let url = Url::parse(&url)?.to_string();
        Ok(Self(json!({
            "tag": "button",
            "text": { "tag": "plain_text", "content": label.into() },
            "type": "default",
            "behaviors": [{ "type": "open_url", "default_url": url }]
        })))
    }

    /// Creates an element from an official CardKit JSON object.
    pub fn raw(value: Value) -> Result<Self> {
        let object = value.as_object().ok_or_else(|| {
            Error::Validation("raw card element must be a JSON object".to_owned())
        })?;
        if object.get("tag").and_then(Value::as_str).is_none() {
            return Err(Error::Validation(
                "raw card element must contain a string tag".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    /// Assigns the CardKit component identifier used by later update APIs.
    pub fn element_id(mut self, element_id: impl Into<String>) -> Result<Self> {
        let element_id = element_id.into();
        validate_element_id(&element_id)?;
        self.object_mut()?
            .insert("element_id".to_owned(), Value::String(element_id));
        Ok(self)
    }

    /// Changes the style of a button element.
    pub fn button_style(mut self, style: CardButtonStyle) -> Result<Self> {
        let object = self.object_mut()?;
        if object.get("tag").and_then(Value::as_str) != Some("button") {
            return Err(Error::Validation(
                "card button style is only supported for button elements".to_owned(),
            ));
        }
        object.insert("type".to_owned(), Value::String(style.as_str().to_owned()));
        Ok(self)
    }

    /// Returns the element JSON value.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// Consumes the element and returns its JSON value.
    pub fn into_value(self) -> Value {
        self.0
    }

    fn object_mut(&mut self) -> Result<&mut Map<String, Value>> {
        self.0
            .as_object_mut()
            .ok_or_else(|| Error::Validation("card element must be a JSON object".to_owned()))
    }
}

impl Serialize for CardElement {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CardElement {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::raw(Value::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Fluent builder for a CardKit 2.0 card.
#[derive(Debug, Clone)]
pub struct CardBuilder {
    config: Map<String, Value>,
    header_title: Option<String>,
    header_subtitle: Option<String>,
    header_template: Option<String>,
    elements: Vec<Value>,
}

impl Default for CardBuilder {
    fn default() -> Self {
        let mut config = Map::new();
        config.insert("update_multi".to_owned(), Value::Bool(true));
        Self {
            config,
            header_title: None,
            header_subtitle: None,
            header_template: None,
            elements: Vec::new(),
        }
    }
}

impl CardBuilder {
    /// Creates an empty builder with `config.update_multi=true`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the card header title.
    pub fn header(mut self, title: impl Into<String>) -> Self {
        self.header_title = Some(title.into());
        self
    }

    /// Sets the optional card header subtitle.
    pub fn header_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.header_subtitle = Some(subtitle.into());
        self
    }

    /// Sets the official CardKit header color template name.
    pub fn header_template(mut self, template: impl Into<String>) -> Self {
        self.header_template = Some(template.into());
        self
    }

    /// Sets a raw CardKit config field.
    pub fn config(mut self, key: impl Into<String>, value: Value) -> Self {
        self.config.insert(key.into(), value);
        self
    }

    /// Appends a typed or raw CardKit body element.
    pub fn element(mut self, element: CardElement) -> Self {
        self.elements.push(element.into_value());
        self
    }

    /// Appends a native Markdown component.
    pub fn markdown(self, content: impl Into<String>) -> Self {
        self.element(CardElement::markdown(content))
    }

    /// Appends a plain-text component.
    pub fn text(self, content: impl Into<String>) -> Self {
        self.element(CardElement::text(content))
    }

    /// Appends a divider component.
    pub fn divider(self) -> Self {
        self.element(CardElement::divider())
    }

    /// Builds and validates the complete CardKit 2.0 card.
    pub fn build(self) -> Result<Card> {
        if self.header_title.is_none()
            && (self.header_subtitle.is_some() || self.header_template.is_some())
        {
            return Err(Error::Validation(
                "card header subtitle or template requires a header title".to_owned(),
            ));
        }

        let header = self.header_title.map(|title| {
            let mut header = Map::new();
            header.insert(
                "title".to_owned(),
                json!({ "tag": "plain_text", "content": title }),
            );
            if let Some(subtitle) = self.header_subtitle {
                header.insert(
                    "subtitle".to_owned(),
                    json!({ "tag": "plain_text", "content": subtitle }),
                );
            }
            if let Some(template) = self.header_template {
                header.insert("template".to_owned(), Value::String(template));
            }
            Value::Object(header)
        });

        Card::from_parts(self.config, header, self.elements)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn builds_common_cardkit_v2_components() {
        let callback = CardElement::callback_button("Approve", json!({ "choice": "approve" }))
            .expect("callback button")
            .button_style(CardButtonStyle::Primary)
            .expect("button style")
            .element_id("approve_button")
            .expect("element id");
        let link =
            CardElement::open_url_button("Docs", "https://open.feishu.cn").expect("link button");

        let card = Card::builder()
            .header("Deployment")
            .header_subtitle("Production")
            .header_template("blue")
            .markdown("**Ready**")
            .text("Review the change")
            .divider()
            .element(callback)
            .element(link)
            .build()
            .expect("card");

        assert_eq!(card.as_value()["schema"], "2.0");
        assert_eq!(card.as_value()["config"]["update_multi"], true);
        assert_eq!(card.as_value()["header"]["title"]["content"], "Deployment");
        assert_eq!(card.as_value()["body"]["elements"][0]["tag"], "markdown");
        assert_eq!(
            card.as_value()["body"]["elements"][3]["behaviors"][0],
            json!({ "type": "callback", "value": { "choice": "approve" } })
        );
        assert_eq!(
            card.as_value()["body"]["elements"][4]["behaviors"][0]["default_url"],
            "https://open.feishu.cn/"
        );
    }

    #[test]
    fn rejects_non_object_callback_values() {
        let error = CardElement::callback_button("Approve", json!("approve"))
            .expect_err("string callback value must fail");

        assert!(matches!(
            error,
            Error::Validation(message)
                if message == "card callback button value must be a JSON object"
        ));
    }

    #[test]
    fn rejects_invalid_element_ids() {
        let error = CardElement::markdown("hello")
            .element_id("1-invalid")
            .expect_err("invalid id must fail");

        assert!(matches!(error, Error::Validation(_)));
    }

    #[test]
    fn rejects_empty_cards() {
        let error = Card::builder().build().expect_err("empty card must fail");

        assert!(matches!(
            error,
            Error::Validation(message)
                if message == "card must contain a header or at least one body element"
        ));
    }
}
