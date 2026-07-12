use std::collections::BTreeMap;

use serde::Serialize;

#[cfg(feature = "websocket")]
use crate::Result;
use crate::card::Card;
#[cfg(feature = "websocket")]
use crate::lark_openapi::WebSocketEventAck;

/// Toast styles supported by a `card.action.trigger` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CardActionToastType {
    Info,
    Success,
    Error,
    Warning,
}

/// A client toast shown after a card interaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CardActionToast {
    #[serde(rename = "type")]
    toast_type: CardActionToastType,
    content: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    i18n: BTreeMap<String, String>,
}

impl CardActionToast {
    /// Creates a toast with default-language content.
    pub fn new(toast_type: CardActionToastType, content: impl Into<String>) -> Self {
        Self {
            toast_type,
            content: content.into(),
            i18n: BTreeMap::new(),
        }
    }

    /// Adds localized content under an official locale key such as `zh_cn`.
    pub fn localized(mut self, locale: impl Into<String>, content: impl Into<String>) -> Self {
        self.i18n.insert(locale.into(), content.into());
        self
    }

    pub fn toast_type(&self) -> CardActionToastType {
        self.toast_type
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn i18n(&self) -> &BTreeMap<String, String> {
        &self.i18n
    }
}

/// Immediate response body for a `card.action.trigger` callback.
///
/// Returning an empty response acknowledges the interaction without changing
/// the card. A toast and a validated CardKit 2.0 card can be attached when the
/// interaction should provide immediate feedback within the callback deadline.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CardActionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    toast: Option<CardActionToast>,
    #[serde(skip_serializing_if = "Option::is_none")]
    card: Option<CardActionResponseCard>,
}

impl CardActionResponse {
    /// Creates an empty successful callback response.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a client toast to the callback response.
    pub fn with_toast(mut self, toast: CardActionToast) -> Self {
        self.toast = Some(toast);
        self
    }

    /// Immediately replaces the interacted card with a validated CardKit card.
    pub fn with_card(mut self, card: Card) -> Self {
        self.card = Some(CardActionResponseCard {
            card_type: "raw",
            data: card,
        });
        self
    }

    pub fn is_empty(&self) -> bool {
        self.toast.is_none() && self.card.is_none()
    }

    /// Encodes this callback response as the Base64 JSON data of a successful
    /// WebSocket event ACK.
    #[cfg(feature = "websocket")]
    pub fn to_websocket_ack(&self) -> Result<WebSocketEventAck> {
        WebSocketEventAck::ok().with_json_data(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct CardActionResponseCard {
    #[serde(rename = "type")]
    card_type: &'static str,
    data: Card,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serializes_empty_callback_response() {
        let response = CardActionResponse::new();

        assert!(response.is_empty());
        assert_eq!(serde_json::to_value(response).expect("response"), json!({}));
    }

    #[test]
    fn serializes_toast_and_cardkit_update() {
        let card = Card::builder().markdown("Approved").build().expect("card");
        let response = CardActionResponse::new()
            .with_toast(
                CardActionToast::new(CardActionToastType::Success, "Approved")
                    .localized("zh_cn", "已批准"),
            )
            .with_card(card);

        assert_eq!(
            serde_json::to_value(response).expect("response"),
            json!({
                "toast": {
                    "type": "success",
                    "content": "Approved",
                    "i18n": { "zh_cn": "已批准" }
                },
                "card": {
                    "type": "raw",
                    "data": {
                        "schema": "2.0",
                        "config": { "update_multi": true },
                        "body": {
                            "elements": [{ "tag": "markdown", "content": "Approved" }]
                        }
                    }
                }
            })
        );
    }

    #[cfg(feature = "websocket")]
    #[test]
    fn encodes_empty_response_as_websocket_ack_data() {
        let ack = CardActionResponse::new()
            .to_websocket_ack()
            .expect("callback ack");

        assert_eq!(ack.code(), 200);
        assert_eq!(ack.data(), Some("e30="));
    }
}
