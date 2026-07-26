use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;
use crate::debug::{JsonSummary, OptionalJsonSummary, RedactedOption};
use crate::message::NormalizedMessage;

mod card_action;
#[cfg(feature = "websocket")]
mod consumer;
#[cfg(feature = "websocket")]
mod consumer_runtime;
mod lark;
#[cfg(feature = "websocket")]
mod r#loop;
mod message_normalization;
#[cfg(feature = "websocket")]
mod reassembly;
#[cfg(feature = "websocket")]
mod runtime;

pub use card_action::{CardActionResponse, CardActionToast, CardActionToastType};
#[cfg(feature = "websocket")]
pub use consumer::{EventConnection, EventConnectionItem, EventConsumer, ReceivedEvent};
#[cfg(feature = "websocket")]
pub use r#loop::{
    EventLoop, EventLoopExit, EventLoopOptions, EventReconnectLimit, EventStreamConnector,
    OpenApiWebSocketEventConnector, WebSocketEndpointConnector,
};
#[cfg(feature = "websocket")]
pub use reassembly::EventPacketReassemblyOptions;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventContext {
    pub event_id: String,
    pub tenant_key: Option<String>,
    pub create_time: Option<String>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ChannelEvent {
    Message(Box<NormalizedMessage>),
    CardAction(Box<CardActionEvent>),
    Unknown {
        context: Option<EventContext>,
        raw: Value,
    },
}

impl fmt::Debug for ChannelEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => formatter.debug_tuple("Message").field(message).finish(),
            Self::CardAction(event) => formatter.debug_tuple("CardAction").field(event).finish(),
            Self::Unknown { context, raw } => formatter
                .debug_struct("Unknown")
                .field("context", context)
                .field("raw", &JsonSummary(raw))
                .finish(),
        }
    }
}

impl ChannelEvent {
    pub fn parse_lark_payload(payload: &[u8]) -> Result<Self> {
        parse_lark_event_payload(payload)
    }
}

pub fn parse_lark_event_payload(payload: &[u8]) -> Result<ChannelEvent> {
    lark::parse_lark_event_payload(payload)
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CardActionEvent {
    pub context: EventContext,
    pub operator: CardActionOperator,
    pub token: Option<String>,
    pub action: CardActionPayload,
    pub host: Option<String>,
    pub delivery_type: Option<String>,
    pub card_context: Option<CardActionContext>,
    pub raw: Value,
}

impl fmt::Debug for CardActionEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CardActionEvent")
            .field("context", &self.context)
            .field("operator", &self.operator)
            .field("token", &RedactedOption(&self.token))
            .field("action", &self.action)
            .field("host", &self.host)
            .field("delivery_type", &self.delivery_type)
            .field("card_context", &self.card_context)
            .field("raw", &JsonSummary(&self.raw))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardActionOperator {
    pub tenant_key: Option<String>,
    pub user_id: Option<String>,
    pub open_id: Option<String>,
    pub union_id: Option<String>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CardActionPayload {
    pub value: Value,
    pub tag: Option<String>,
    pub timezone: Option<String>,
    pub name: Option<String>,
    pub form_value: Option<Value>,
    pub input_value: Option<String>,
    pub option: Option<String>,
    pub options: Vec<String>,
    pub checked: Option<bool>,
    pub raw: Value,
}

impl fmt::Debug for CardActionPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CardActionPayload")
            .field("value", &JsonSummary(&self.value))
            .field("tag", &self.tag)
            .field("timezone", &self.timezone)
            .field("name", &self.name)
            .field("form_value", &OptionalJsonSummary(&self.form_value))
            .field(
                "input_value_chars",
                &self.input_value.as_ref().map(|value| value.chars().count()),
            )
            .field("option_present", &self.option.is_some())
            .field("options_len", &self.options.len())
            .field("checked", &self.checked)
            .field("raw", &JsonSummary(&self.raw))
            .finish()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CardActionContext {
    pub url: Option<String>,
    pub preview_token: Option<String>,
    pub open_message_id: Option<String>,
    pub open_chat_id: Option<String>,
    pub raw: Value,
}

impl fmt::Debug for CardActionContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CardActionContext")
            .field("url", &RedactedOption(&self.url))
            .field("preview_token", &RedactedOption(&self.preview_token))
            .field("open_message_id", &self.open_message_id)
            .field("open_chat_id", &self.open_chat_id)
            .field("raw", &JsonSummary(&self.raw))
            .finish()
    }
}
