use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::{
    MessageChatType, MessageMention, MessageSenderInfo, MessageSenderType, NormalizedMessage,
};
use crate::{Error, Result};

#[cfg(feature = "websocket")]
mod consumer;
#[cfg(feature = "websocket")]
mod r#loop;
#[cfg(feature = "websocket")]
mod reassembly;

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
    Message(NormalizedMessage),
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

    if header.event_type == "card.action.trigger" {
        let event = raw.get("event").cloned().ok_or_else(|| {
            Error::Validation("lark card action callback is missing event body".to_owned())
        })?;
        let event = serde_json::from_value::<LarkCardActionEvent>(event)?;
        return event.into_channel_event(context, raw);
    }

    Ok(ChannelEvent::Unknown {
        context: Some(context),
        raw,
    })
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

#[derive(Debug, Deserialize)]
struct LarkCardActionEvent {
    #[serde(default)]
    operator: LarkCardActionOperator,
    #[serde(default)]
    token: Option<String>,
    action: Value,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    delivery_type: Option<String>,
    #[serde(default)]
    context: Option<Value>,
}

impl LarkCardActionEvent {
    fn into_channel_event(self, context: EventContext, raw: Value) -> Result<ChannelEvent> {
        Ok(ChannelEvent::CardAction(Box::new(CardActionEvent {
            context,
            operator: self.operator.into(),
            token: self.token,
            action: CardActionPayload::from_lark(self.action)?,
            host: self.host,
            delivery_type: self.delivery_type,
            card_context: self.context.map(CardActionContext::from_lark).transpose()?,
            raw,
        })))
    }
}

#[derive(Debug, Default, Deserialize)]
struct LarkCardActionOperator {
    #[serde(default)]
    tenant_key: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    open_id: Option<String>,
    #[serde(default)]
    union_id: Option<String>,
}

impl From<LarkCardActionOperator> for CardActionOperator {
    fn from(value: LarkCardActionOperator) -> Self {
        Self {
            tenant_key: value.tenant_key,
            user_id: value.user_id,
            open_id: value.open_id,
            union_id: value.union_id,
        }
    }
}

#[derive(Debug, Deserialize)]
struct LarkCardActionPayload {
    #[serde(default)]
    value: Value,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    form_value: Option<Value>,
    #[serde(default)]
    input_value: Option<String>,
    #[serde(default)]
    option: Option<String>,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    checked: Option<bool>,
}

impl CardActionPayload {
    fn from_lark(raw: Value) -> Result<Self> {
        let value = serde_json::from_value::<LarkCardActionPayload>(raw.clone())?;
        Ok(Self {
            value: value.value,
            tag: value.tag,
            timezone: value.timezone,
            name: value.name,
            form_value: value.form_value,
            input_value: value.input_value,
            option: value.option,
            options: value.options,
            checked: value.checked,
            raw,
        })
    }
}

#[derive(Debug, Deserialize)]
struct LarkCardActionContext {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    preview_token: Option<String>,
    #[serde(default)]
    open_message_id: Option<String>,
    #[serde(default)]
    open_chat_id: Option<String>,
}

impl CardActionContext {
    fn from_lark(raw: Value) -> Result<Self> {
        let value = serde_json::from_value::<LarkCardActionContext>(raw.clone())?;
        Ok(Self {
            url: value.url,
            preview_token: value.preview_token,
            open_message_id: value.open_message_id,
            open_chat_id: value.open_chat_id,
            raw,
        })
    }
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

    #[test]
    fn parses_lark_card_action_trigger_callback() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_card_1",
                "event_type": "card.action.trigger",
                "create_time": "1603977298000000",
                "tenant_key": "tenant_1",
                "app_id": "cli_1"
            },
            "event": {
                "operator": {
                    "tenant_key": "tenant_1",
                    "user_id": "user_1",
                    "open_id": "ou_1",
                    "union_id": "on_1"
                },
                "token": "card_update_token",
                "action": {
                    "value": {
                        "choice": "approve",
                        "ticket_id": "ticket_1"
                    },
                    "tag": "button",
                    "timezone": "Asia/Shanghai",
                    "name": "Button_1",
                    "form_value": {
                        "reason": "looks good"
                    },
                    "input_value": "typed text",
                    "option": "option_1",
                    "options": ["option_1", "option_2"],
                    "checked": true,
                    "extra_field": "preserved"
                },
                "host": "im_message",
                "delivery_type": "url_preview",
                "context": {
                    "url": "https://example.test",
                    "preview_token": "preview_1",
                    "open_message_id": "om_1",
                    "open_chat_id": "oc_1",
                    "extra_context": "preserved"
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::CardAction(card_action) = event else {
            panic!("expected card action event");
        };

        assert_eq!(card_action.context.event_id, "event_card_1");
        assert_eq!(card_action.context.tenant_key.as_deref(), Some("tenant_1"));
        assert_eq!(
            card_action.context.create_time.as_deref(),
            Some("1603977298000000")
        );
        assert_eq!(card_action.operator.tenant_key.as_deref(), Some("tenant_1"));
        assert_eq!(card_action.operator.user_id.as_deref(), Some("user_1"));
        assert_eq!(card_action.operator.open_id.as_deref(), Some("ou_1"));
        assert_eq!(card_action.operator.union_id.as_deref(), Some("on_1"));
        assert_eq!(card_action.token.as_deref(), Some("card_update_token"));
        assert_eq!(card_action.action.value["choice"], "approve");
        assert_eq!(card_action.action.value["ticket_id"], "ticket_1");
        assert_eq!(card_action.action.tag.as_deref(), Some("button"));
        assert_eq!(
            card_action.action.timezone.as_deref(),
            Some("Asia/Shanghai")
        );
        assert_eq!(card_action.action.name.as_deref(), Some("Button_1"));
        assert_eq!(
            card_action.action.form_value.as_ref().expect("form value")["reason"],
            "looks good"
        );
        assert_eq!(
            card_action.action.input_value.as_deref(),
            Some("typed text")
        );
        assert_eq!(card_action.action.option.as_deref(), Some("option_1"));
        assert_eq!(card_action.action.options, vec!["option_1", "option_2"]);
        assert_eq!(card_action.action.checked, Some(true));
        assert_eq!(card_action.action.raw["extra_field"], "preserved");
        assert_eq!(card_action.host.as_deref(), Some("im_message"));
        assert_eq!(card_action.delivery_type.as_deref(), Some("url_preview"));
        let card_context = card_action.card_context.expect("card context");
        assert_eq!(card_context.url.as_deref(), Some("https://example.test"));
        assert_eq!(card_context.preview_token.as_deref(), Some("preview_1"));
        assert_eq!(card_context.open_message_id.as_deref(), Some("om_1"));
        assert_eq!(card_context.open_chat_id.as_deref(), Some("oc_1"));
        assert_eq!(card_context.raw["extra_context"], "preserved");
        assert_eq!(
            card_action.raw["header"]["event_type"],
            "card.action.trigger"
        );
    }

    #[test]
    fn parses_lark_card_action_string_value() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_card_2",
                "event_type": "card.action.trigger"
            },
            "event": {
                "operator": {
                    "open_id": "ou_1"
                },
                "action": {
                    "value": "plain-value",
                    "tag": "select_static"
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::CardAction(card_action) = event else {
            panic!("expected card action event");
        };

        assert_eq!(card_action.operator.open_id.as_deref(), Some("ou_1"));
        assert_eq!(
            card_action.action.value,
            Value::String("plain-value".to_owned())
        );
        assert_eq!(card_action.action.tag.as_deref(), Some("select_static"));
        assert_eq!(card_action.action.raw["value"], "plain-value");
    }

    #[test]
    fn card_action_requires_event_body() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_card_3",
                "event_type": "card.action.trigger"
            }
        });

        let error = parse_lark_event_payload(payload.to_string().as_bytes())
            .expect_err("missing card callback event body should fail");
        assert!(
            matches!(error, Error::Validation(message) if message.contains("card action callback"))
        );
    }
}
