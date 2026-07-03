use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::{
    MessageChatType, MessageMention, MessageSenderInfo, MessageSenderType, NormalizedMessage,
};
use crate::{Error, Result};

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
    CardAction {
        context: EventContext,
        action: Value,
    },
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
    let raw: Value = serde_json::from_slice(payload)?;
    let header = raw
        .get("header")
        .cloned()
        .map(serde_json::from_value::<LarkEventHeader>)
        .transpose()?
        .ok_or_else(|| Error::Validation("lark event payload is missing header".to_owned()))?;
    let context = header.context();

    if header.event_type == "im.message.receive_v1" {
        return raw
            .get("event")
            .cloned()
            .map(serde_json::from_value::<LarkMessageReceiveEvent>)
            .transpose()?
            .map(|event| ChannelEvent::Message(event.into_normalized_message(raw)))
            .ok_or_else(|| {
                Error::Validation("lark message receive event is missing event body".to_owned())
            });
    }

    Ok(ChannelEvent::Unknown {
        context: Some(context),
        raw,
    })
}

impl LarkEventHeader {
    fn context(&self) -> EventContext {
        EventContext {
            event_id: self.event_id.clone(),
            tenant_key: self.tenant_key.clone(),
            create_time: self.create_time.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct LarkEventHeader {
    event_id: String,
    event_type: String,
    #[serde(default)]
    create_time: Option<String>,
    #[serde(default)]
    tenant_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LarkMessageReceiveEvent {
    sender: LarkEventSender,
    message: LarkEventMessage,
}

impl LarkMessageReceiveEvent {
    fn into_normalized_message(self, raw: Value) -> NormalizedMessage {
        let sender_open_id = self.sender.sender_id.open_id;
        let sender_type = parse_sender_type(&self.sender.sender_type);
        let text = parse_text_content(&self.message.content);
        let mentions = self
            .message
            .mentions
            .into_iter()
            .map(|mention| MessageMention {
                key: mention.key,
                open_id: mention.id.open_id,
                name: mention.name,
                mentioned_type: mention
                    .mentioned_type
                    .as_deref()
                    .map(parse_sender_type)
                    .unwrap_or(MessageSenderType::Unknown),
            })
            .collect();

        NormalizedMessage {
            message_id: self.message.message_id,
            chat_id: self.message.chat_id,
            chat_type: parse_chat_type(&self.message.chat_type),
            sender_id: sender_open_id.clone(),
            sender: MessageSenderInfo {
                open_id: sender_open_id,
                sender_type,
            },
            message_type: self.message.message_type,
            text,
            root_id: empty_string_as_none(self.message.root_id),
            parent_id: empty_string_as_none(self.message.parent_id),
            thread_id: empty_string_as_none(self.message.thread_id),
            mentions,
            raw,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LarkEventSender {
    sender_id: LarkUserId,
    sender_type: String,
}

#[derive(Debug, Deserialize)]
struct LarkEventMessage {
    message_id: String,
    chat_id: String,
    chat_type: String,
    message_type: String,
    content: String,
    #[serde(default)]
    root_id: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    mentions: Vec<LarkEventMention>,
}

#[derive(Debug, Deserialize)]
struct LarkEventMention {
    key: String,
    id: LarkUserId,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mentioned_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LarkUserId {
    open_id: String,
}

fn parse_chat_type(value: &str) -> MessageChatType {
    match value {
        "p2p" => MessageChatType::P2p,
        "group" => MessageChatType::Group,
        _ => MessageChatType::Unknown,
    }
}

fn parse_sender_type(value: &str) -> MessageSenderType {
    match value {
        "user" => MessageSenderType::User,
        "bot" => MessageSenderType::Bot,
        _ => MessageSenderType::Unknown,
    }
}

fn parse_text_content(content: &str) -> String {
    serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|value| value.get("text").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

fn empty_string_as_none(value: Option<String>) -> Option<String> {
    value.and_then(|value| (!value.is_empty()).then_some(value))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_lark_message_receive_event() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_1",
                "event_type": "im.message.receive_v1",
                "create_time": "1608725989000",
                "tenant_key": "tenant_1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_1",
                    "root_id": "om_root",
                    "parent_id": "om_parent",
                    "thread_id": "omt_1",
                    "chat_id": "oc_1",
                    "chat_type": "group",
                    "message_type": "text",
                    "content": "{\"text\":\"@_user_1 hello\"}",
                    "mentions": [{
                        "key": "@_user_1",
                        "id": {
                            "open_id": "ou_bot"
                        },
                        "name": "Bot"
                    }]
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.message_id, "om_1");
        assert_eq!(message.chat_id, "oc_1");
        assert_eq!(message.chat_type, MessageChatType::Group);
        assert_eq!(message.sender_id, "ou_sender");
        assert_eq!(message.sender.sender_type, MessageSenderType::User);
        assert_eq!(message.message_type, "text");
        assert_eq!(message.text, "@_user_1 hello");
        assert_eq!(message.root_id.as_deref(), Some("om_root"));
        assert_eq!(message.parent_id.as_deref(), Some("om_parent"));
        assert_eq!(message.thread_id.as_deref(), Some("omt_1"));
        assert_eq!(message.mentions.len(), 1);
        assert_eq!(
            message.mentions[0].mentioned_type,
            MessageSenderType::Unknown
        );
        assert!(message.mentions_bot("ou_bot"));
    }

    #[test]
    fn unknown_lark_event_preserves_context_and_raw_payload() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_2",
                "event_type": "im.chat.member.user.added_v1"
            },
            "event": {
                "chat_id": "oc_1"
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Unknown { context, raw } = event else {
            panic!("expected unknown event");
        };

        assert_eq!(
            context.as_ref().map(|context| context.event_id.as_str()),
            Some("event_2")
        );
        assert_eq!(raw["event"]["chat_id"], "oc_1");
    }
}
