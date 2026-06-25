use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::NormalizedMessage;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventContext {
    pub event_id: String,
    pub tenant_key: Option<String>,
    pub create_time: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ChannelEvent {
    Message(NormalizedMessage),
    CardAction { context: EventContext, action: Value },
    Unknown { context: Option<EventContext>, raw: Value },
}
