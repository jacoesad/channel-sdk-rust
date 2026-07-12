use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ChannelEvent {
    Message(Box<NormalizedMessage>),
    CardAction(Box<CardActionEvent>),
    Unknown {
        context: Option<EventContext>,
        raw: Value,
    },
}

impl ChannelEvent {
    pub fn parse_lark_payload(payload: &[u8]) -> Result<Self> {
        parse_lark_event_payload(payload)
    }
}

pub fn parse_lark_event_payload(payload: &[u8]) -> Result<ChannelEvent> {
    lark::parse_lark_event_payload(payload)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardActionOperator {
    pub tenant_key: Option<String>,
    pub user_id: Option<String>,
    pub open_id: Option<String>,
    pub union_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardActionContext {
    pub url: Option<String>,
    pub preview_token: Option<String>,
    pub open_message_id: Option<String>,
    pub open_chat_id: Option<String>,
    pub raw: Value,
}
