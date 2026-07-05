use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::message::{
    MessageChatType, MessageMention, MessageSenderInfo, MessageSenderType, NormalizedMessage,
};
use crate::{Error, Result};

#[cfg(feature = "websocket")]
mod consumer;
#[cfg(feature = "websocket")]
mod consumer_runtime;
#[cfg(feature = "websocket")]
mod r#loop;
#[cfg(feature = "websocket")]
mod reassembly;
#[cfg(feature = "websocket")]
mod runtime;

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
            .map(|event| ChannelEvent::Message(Box::new(event.into_normalized_message(raw))))
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
        let sender_id = self.sender.sender_id;
        let sender_open_id = sender_id.open_id.clone();
        let sender_type = parse_sender_type(&self.sender.sender_type);
        let content = parse_message_content(&self.message.content);
        let event_mentions = self
            .message
            .mentions
            .into_iter()
            .map(LarkEventMention::into_message_mention)
            .collect::<Vec<_>>();
        let mentions = normalize_message_mentions(
            &self.message.message_type,
            content.parsed.as_ref(),
            event_mentions.clone(),
        );
        let text = normalize_message_text(
            &self.message.message_type,
            content.parsed.as_ref(),
            &event_mentions,
        );

        NormalizedMessage {
            message_id: self.message.message_id,
            chat_id: self.message.chat_id,
            chat_type: parse_chat_type(&self.message.chat_type),
            sender_id: sender_open_id.clone(),
            sender: MessageSenderInfo {
                open_id: sender_open_id,
                user_id: sender_id.user_id,
                union_id: sender_id.union_id,
                sender_type,
            },
            message_type: self.message.message_type,
            text,
            raw_content: content.raw,
            content: content.parsed,
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
    #[serde(default)]
    chat_type: String,
    #[serde(default)]
    message_type: String,
    #[serde(default)]
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
    #[serde(default)]
    key: String,
    #[serde(default, deserialize_with = "deserialize_lark_mention_id")]
    id: LarkMentionId,
    #[serde(default)]
    id_type: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mentioned_type: Option<String>,
}

impl LarkEventMention {
    fn into_message_mention(self) -> MessageMention {
        let (open_id, user_id, union_id) = self.id.into_parts(self.id_type.as_deref());

        MessageMention {
            key: self.key,
            open_id,
            user_id,
            union_id,
            name: self.name,
            mentioned_type: self
                .mentioned_type
                .as_deref()
                .map(parse_sender_type)
                .unwrap_or(MessageSenderType::Unknown),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct LarkUserId {
    #[serde(default)]
    open_id: String,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    union_id: Option<String>,
}

#[derive(Debug, Default)]
struct LarkMentionId {
    open_id: String,
    user_id: Option<String>,
    union_id: Option<String>,
    raw_id: Option<String>,
}

impl LarkMentionId {
    fn into_parts(self, id_type: Option<&str>) -> (String, Option<String>, Option<String>) {
        let user_id = empty_string_as_none(self.user_id);
        let union_id = empty_string_as_none(self.union_id);
        if !self.open_id.is_empty() || user_id.is_some() || union_id.is_some() {
            return (self.open_id, user_id, union_id);
        }

        let Some(raw_id) = self.raw_id.filter(|id| !id.is_empty()) else {
            return (String::new(), None, None);
        };

        match id_type {
            Some("user_id") => (String::new(), Some(raw_id), None),
            Some("union_id") => (String::new(), None, Some(raw_id)),
            _ => (raw_id, None, None),
        }
    }
}

fn deserialize_lark_mention_id<'de, D>(
    deserializer: D,
) -> std::result::Result<LarkMentionId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MentionIdValue {
        Structured(LarkUserId),
        Raw(String),
    }

    Option::<MentionIdValue>::deserialize(deserializer).map(|value| match value {
        Some(MentionIdValue::Structured(id)) => LarkMentionId {
            open_id: id.open_id,
            user_id: id.user_id,
            union_id: id.union_id,
            raw_id: None,
        },
        Some(MentionIdValue::Raw(raw_id)) => LarkMentionId {
            raw_id: Some(raw_id),
            ..LarkMentionId::default()
        },
        None => LarkMentionId::default(),
    })
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

struct ParsedMessageContent {
    raw: String,
    parsed: Option<Value>,
}

fn parse_message_content(content: &str) -> ParsedMessageContent {
    let parsed = if content.is_empty() {
        None
    } else {
        serde_json::from_str::<Value>(content).ok()
    };

    ParsedMessageContent {
        raw: content.to_owned(),
        parsed,
    }
}

fn normalize_message_mentions(
    message_type: &str,
    content: Option<&Value>,
    event_mentions: Vec<MessageMention>,
) -> Vec<MessageMention> {
    let mut mentions = Vec::new();

    for mention in event_mentions {
        push_message_mention(&mut mentions, mention);
    }

    if message_type == "post" {
        for mention in parse_post_mentions(content) {
            push_message_mention(&mut mentions, mention);
        }
    }

    mentions
}

fn normalize_message_text(
    message_type: &str,
    content: Option<&Value>,
    mentions: &[MessageMention],
) -> String {
    let text = match message_type {
        "text" => parse_text_content(content),
        "post" => parse_post_content(content),
        _ => String::new(),
    };

    resolve_mention_keys(text, mentions)
}

fn parse_text_content(content: Option<&Value>) -> String {
    content
        .and_then(|value| value.get("text").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

fn parse_post_content(content: Option<&Value>) -> String {
    content
        .and_then(select_post_document)
        .map(post_document_text)
        .unwrap_or_default()
}

fn parse_post_mentions(content: Option<&Value>) -> Vec<MessageMention> {
    let Some(document) = content.and_then(select_post_document) else {
        return Vec::new();
    };

    let Some(content) = document.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut mentions = Vec::new();
    for line in content {
        let Some(elements) = line.as_array() else {
            continue;
        };
        for element in elements {
            if let Some(mention) = post_at_mention(element) {
                push_message_mention(&mut mentions, mention);
            }
        }
    }

    mentions
}

fn select_post_document(value: &Value) -> Option<&Value> {
    if is_post_document(value) {
        return Some(value);
    }

    for locale in ["zh_cn", "en_us", "ja_jp"] {
        if let Some(document) = value.get(locale).filter(|value| is_post_document(value)) {
            return Some(document);
        }
    }

    value
        .as_object()?
        .values()
        .find(|value| is_post_document(value))
}

fn is_post_document(value: &Value) -> bool {
    value.get("title").and_then(Value::as_str).is_some()
        || value.get("content").and_then(Value::as_array).is_some()
}

fn post_document_text(document: &Value) -> String {
    let mut lines = Vec::new();

    if let Some(title) = document.get("title").and_then(Value::as_str) {
        if !title.is_empty() {
            lines.push(title.to_owned());
        }
    }

    if let Some(content) = document.get("content").and_then(Value::as_array) {
        lines.extend(
            content
                .iter()
                .filter_map(post_line_text)
                .filter(|line| !line.is_empty()),
        );
    }

    lines.join("\n")
}

fn post_line_text(line: &Value) -> Option<String> {
    let elements = line.as_array()?;
    let mut text = String::new();

    for element in elements {
        text.push_str(&post_element_text(element));
    }

    Some(text)
}

fn post_element_text(element: &Value) -> String {
    match element.get("tag").and_then(Value::as_str) {
        Some("at") => post_at_text(element),
        Some("text" | "a") => element_text(element),
        _ => element_text(element),
    }
}

fn element_text(element: &Value) -> String {
    element
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn post_at_text(element: &Value) -> String {
    let name = element
        .get("user_name")
        .or_else(|| element.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if name.is_empty() || name.starts_with('@') {
        name.to_owned()
    } else {
        format!("@{name}")
    }
}

fn post_at_mention(element: &Value) -> Option<MessageMention> {
    if element.get("tag").and_then(Value::as_str) != Some("at") {
        return None;
    }

    let raw_user_id = element
        .get("user_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = element
        .get("user_name")
        .or_else(|| element.get("text"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);

    if raw_user_id.is_empty() {
        return None;
    }

    if raw_user_id == "all" || raw_user_id == "all_members" {
        return Some(MessageMention {
            key: "@_all".to_owned(),
            open_id: String::new(),
            user_id: None,
            union_id: None,
            name,
            mentioned_type: MessageSenderType::Unknown,
        });
    }

    let (open_id, user_id) = if raw_user_id.starts_with("ou_") {
        (raw_user_id.to_owned(), None)
    } else {
        (String::new(), Some(raw_user_id.to_owned()))
    };

    Some(MessageMention {
        key: String::new(),
        open_id,
        user_id,
        union_id: None,
        name,
        mentioned_type: MessageSenderType::Unknown,
    })
}

fn resolve_mention_keys(text: String, mentions: &[MessageMention]) -> String {
    let replacements = mentions
        .iter()
        .filter(|mention| !mention.key.is_empty())
        .map(|mention| (mention.key.as_str(), mention_display_text(mention)))
        .filter(|(key, replacement)| key != replacement)
        .collect::<Vec<_>>();

    if replacements.is_empty() {
        return text;
    }

    let mut resolved = String::with_capacity(text.len());
    let mut index = 0;

    while index < text.len() {
        let remaining = &text[index..];
        if let Some((key, replacement)) = replacements
            .iter()
            .filter(|(key, _)| remaining_starts_with_mention_key(remaining, key))
            .max_by_key(|(key, _)| key.len())
        {
            resolved.push_str(replacement);
            index += key.len();
        } else if let Some(next) = remaining.chars().next() {
            resolved.push(next);
            index += next.len_utf8();
        } else {
            break;
        }
    }

    resolved
}

fn remaining_starts_with_mention_key(remaining: &str, key: &str) -> bool {
    if !remaining.starts_with(key) {
        return false;
    }

    match remaining[key.len()..].chars().next() {
        Some(next) => !is_mention_key_char(next),
        None => true,
    }
}

fn is_mention_key_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}

fn mention_display_text(mention: &MessageMention) -> String {
    if let Some(name) = mention.name.as_deref().filter(|name| !name.is_empty()) {
        if name.starts_with('@') {
            name.to_owned()
        } else {
            format!("@{name}")
        }
    } else {
        mention.key.clone()
    }
}

fn push_message_mention(mentions: &mut Vec<MessageMention>, mention: MessageMention) {
    if mention.key.is_empty()
        && mention.open_id.is_empty()
        && mention.user_id.is_none()
        && mention.union_id.is_none()
    {
        return;
    }

    if let Some(existing) = mentions
        .iter_mut()
        .find(|existing| same_message_mention(existing, &mention))
    {
        merge_message_mention(existing, mention);
    } else {
        mentions.push(mention);
    }
}

fn same_message_mention(left: &MessageMention, right: &MessageMention) -> bool {
    (!left.key.is_empty() && left.key == right.key)
        || (!left.open_id.is_empty() && left.open_id == right.open_id)
        || same_non_empty_optional_id(left.user_id.as_deref(), right.user_id.as_deref())
        || same_non_empty_optional_id(left.union_id.as_deref(), right.union_id.as_deref())
}

fn same_non_empty_optional_id(left: Option<&str>, right: Option<&str>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if !left.is_empty() && left == right)
}

fn merge_message_mention(existing: &mut MessageMention, next: MessageMention) {
    if existing.key.is_empty() {
        existing.key = next.key;
    }
    if existing.open_id.is_empty() {
        existing.open_id = next.open_id;
    }
    if existing.user_id.is_none() {
        existing.user_id = next.user_id;
    }
    if existing.union_id.is_none() {
        existing.union_id = next.union_id;
    }
    if existing.name.is_none() {
        existing.name = next.name;
    }
    if existing.mentioned_type == MessageSenderType::Unknown {
        existing.mentioned_type = next.mentioned_type;
    }
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
                        "open_id": "ou_sender",
                        "user_id": "u_sender",
                        "union_id": "on_sender"
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
                            "open_id": "ou_bot",
                            "user_id": "u_bot",
                            "union_id": "on_bot"
                        },
                        "name": "Bot",
                        "mentioned_type": "bot"
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
        assert_eq!(message.sender.open_id, "ou_sender");
        assert_eq!(message.sender.user_id.as_deref(), Some("u_sender"));
        assert_eq!(message.sender.union_id.as_deref(), Some("on_sender"));
        assert_eq!(message.sender.sender_type, MessageSenderType::User);
        assert_eq!(message.message_type, "text");
        assert_eq!(message.text, "@Bot hello");
        assert_eq!(message.raw_content, "{\"text\":\"@_user_1 hello\"}");
        assert_eq!(
            message.content.as_ref().expect("content")["text"],
            "@_user_1 hello"
        );
        assert_eq!(message.root_id.as_deref(), Some("om_root"));
        assert_eq!(message.parent_id.as_deref(), Some("om_parent"));
        assert_eq!(message.thread_id.as_deref(), Some("omt_1"));
        assert_eq!(message.mentions.len(), 1);
        assert_eq!(message.mentions[0].open_id, "ou_bot");
        assert_eq!(message.mentions[0].user_id.as_deref(), Some("u_bot"));
        assert_eq!(message.mentions[0].union_id.as_deref(), Some("on_bot"));
        assert_eq!(message.mentions[0].name.as_deref(), Some("Bot"));
        assert_eq!(message.mentions[0].mentioned_type, MessageSenderType::Bot);
        assert!(message.mentions_bot("ou_bot"));
    }

    #[test]
    fn parses_lark_message_mentions_with_legacy_id_shape() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_legacy_mention_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_legacy_mention",
                    "chat_id": "oc_1",
                    "message_type": "text",
                    "content": "{\"text\":\"hello @_open and @_user\"}",
                    "mentions": [{
                        "key": "@_open",
                        "id": "ou_legacy",
                        "id_type": "open_id",
                        "name": "Legacy"
                    }, {
                        "key": "@_user",
                        "id": "u_legacy",
                        "id_type": "user_id",
                        "name": "UserOnly"
                    }]
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.text, "hello @Legacy and @UserOnly");
        assert_eq!(message.mentions.len(), 2);
        assert_eq!(message.mentions[0].open_id, "ou_legacy");
        assert_eq!(message.mentions[0].name.as_deref(), Some("Legacy"));
        assert_eq!(message.mentions[1].open_id, "");
        assert_eq!(message.mentions[1].user_id.as_deref(), Some("u_legacy"));
        assert_eq!(message.mentions[1].name.as_deref(), Some("UserOnly"));
        assert!(message.mentions_bot("ou_legacy"));
    }

    #[test]
    fn resolves_overlapping_mention_keys_without_corrupting_longer_keys() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_overlapping_mentions_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_overlapping_mentions",
                    "chat_id": "oc_1",
                    "message_type": "text",
                    "content": "{\"text\":\"@_user_10 then @_user_1\"}",
                    "mentions": [{
                        "key": "@_user_1",
                        "id": {
                            "open_id": "ou_short"
                        },
                        "name": "Short"
                    }, {
                        "key": "@_user_10",
                        "id": {
                            "open_id": "ou_long"
                        },
                        "name": "Long"
                    }]
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.text, "@Long then @Short");
        assert_eq!(message.mentions.len(), 2);
        assert_eq!(message.mentions[0].open_id, "ou_short");
        assert_eq!(message.mentions[1].open_id, "ou_long");
    }

    #[test]
    fn resolves_duplicate_mention_aliases_before_deduplicating_public_mentions() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_duplicate_mention_aliases_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_duplicate_mention_aliases",
                    "chat_id": "oc_1",
                    "message_type": "text",
                    "content": "{\"text\":\"@_user_1 then @_user_2\"}",
                    "mentions": [{
                        "key": "@_user_1",
                        "id": {
                            "open_id": "ou_bot"
                        },
                        "name": "Bot"
                    }, {
                        "key": "@_user_2",
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

        assert_eq!(message.text, "@Bot then @Bot");
        assert_eq!(message.mentions.len(), 1);
        assert_eq!(message.mentions[0].key, "@_user_1");
        assert_eq!(message.mentions[0].open_id, "ou_bot");
    }

    #[test]
    fn does_not_resolve_known_mention_key_inside_unknown_longer_key() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_mention_prefix_boundary_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_mention_prefix_boundary",
                    "chat_id": "oc_1",
                    "message_type": "text",
                    "content": "{\"text\":\"@_user_10 then @_user_1\"}",
                    "mentions": [{
                        "key": "@_user_1",
                        "id": {
                            "open_id": "ou_short"
                        },
                        "name": "Short"
                    }]
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.text, "@_user_10 then @Short");
        assert_eq!(message.mentions.len(), 1);
        assert_eq!(message.mentions[0].key, "@_user_1");
    }

    #[test]
    fn normalizes_lark_post_message_to_plain_text() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_post_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_post",
                    "chat_id": "oc_1",
                    "chat_type": "group",
                    "message_type": "post",
                    "content": serde_json::to_string(&json!({
                        "title": "Post title",
                        "content": [
                            [
                                {
                                    "tag": "text",
                                    "text": "hello "
                                },
                                {
                                    "tag": "at",
                                    "user_id": "ou_bot",
                                    "user_name": "Bot"
                                },
                                {
                                    "tag": "text",
                                    "text": " see "
                                },
                                {
                                    "tag": "a",
                                    "text": "docs",
                                    "href": "https://example.test"
                                }
                            ],
                            [
                                {
                                    "tag": "text",
                                    "text": "second line"
                                },
                                {
                                    "tag": "img",
                                    "image_key": "img_v2_1"
                                }
                            ]
                        ]
                    })).expect("post content")
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.message_type, "post");
        assert_eq!(message.text, "Post title\nhello @Bot see docs\nsecond line");
        assert_eq!(
            message.content.as_ref().expect("content")["content"][0][1]["user_name"],
            "Bot"
        );
        assert_eq!(message.mentions.len(), 1);
        assert_eq!(message.mentions[0].open_id, "ou_bot");
        assert_eq!(message.mentions[0].name.as_deref(), Some("Bot"));
        assert!(message.mentions_bot("ou_bot"));
    }

    #[test]
    fn deduplicates_lark_post_mentions_against_event_metadata() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_post_mention_dedupe_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_post_mention_dedupe",
                    "chat_id": "oc_1",
                    "message_type": "post",
                    "content": serde_json::to_string(&json!({
                        "title": "",
                        "content": [[
                            {
                                "tag": "at",
                                "user_id": "ou_bot",
                                "user_name": "Bot"
                            }
                        ]]
                    })).expect("post content"),
                    "mentions": [{
                        "key": "@_user_1",
                        "id": {
                            "open_id": "ou_bot"
                        }
                    }]
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.text, "@Bot");
        assert_eq!(message.mentions.len(), 1);
        assert_eq!(message.mentions[0].key, "@_user_1");
        assert_eq!(message.mentions[0].open_id, "ou_bot");
        assert_eq!(message.mentions[0].name.as_deref(), Some("Bot"));
    }

    #[test]
    fn does_not_deduplicate_distinct_mentions_with_empty_optional_ids() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_empty_optional_mention_ids_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_empty_optional_mention_ids",
                    "chat_id": "oc_1",
                    "message_type": "text",
                    "content": "{\"text\":\"@_a @_b\"}",
                    "mentions": [{
                        "key": "@_a",
                        "id": {
                            "open_id": "ou_a",
                            "user_id": "",
                            "union_id": ""
                        },
                        "name": "A"
                    }, {
                        "key": "@_b",
                        "id": {
                            "open_id": "ou_b",
                            "user_id": "",
                            "union_id": ""
                        },
                        "name": "B"
                    }]
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.text, "@A @B");
        assert_eq!(message.mentions.len(), 2);
        assert_eq!(message.mentions[0].open_id, "ou_a");
        assert_eq!(message.mentions[0].user_id, None);
        assert_eq!(message.mentions[0].union_id, None);
        assert_eq!(message.mentions[1].open_id, "ou_b");
        assert_eq!(message.mentions[1].user_id, None);
        assert_eq!(message.mentions[1].union_id, None);
    }

    #[test]
    fn skips_rich_text_at_mentions_without_identifier() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_post_mention_without_id_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_post_mention_without_id",
                    "chat_id": "oc_1",
                    "message_type": "post",
                    "content": serde_json::to_string(&json!({
                        "title": "",
                        "content": [[
                            {
                                "tag": "at",
                                "user_name": "Visible"
                            }
                        ]]
                    })).expect("post content")
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.text, "@Visible");
        assert!(message.mentions.is_empty());
    }

    #[test]
    fn normalizes_multilingual_lark_post_message_with_preferred_locale() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_post_i18n_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_post_i18n",
                    "chat_id": "oc_1",
                    "message_type": "post",
                    "content": serde_json::to_string(&json!({
                        "en_us": {
                            "title": "English",
                            "content": [[
                                {
                                    "tag": "text",
                                    "text": "hello"
                                }
                            ]]
                        },
                        "zh_cn": {
                            "title": "中文",
                            "content": [[
                                {
                                    "tag": "text",
                                    "text": "你好"
                                }
                            ]]
                        }
                    })).expect("post content")
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.text, "中文\n你好");
    }

    #[test]
    fn normalizes_lark_post_message_with_locale_fallbacks() {
        let ja_jp = json!({
            "ja_jp": {
                "title": "日本語",
                "content": [[
                    {
                        "tag": "text",
                        "text": "こんにちは"
                    }
                ]]
            }
        });

        assert_eq!(parse_post_content(Some(&ja_jp)), "日本語\nこんにちは");

        let en_us_over_ja_jp = json!({
            "ja_jp": {
                "title": "日本語",
                "content": [[
                    {
                        "tag": "text",
                        "text": "こんにちは"
                    }
                ]]
            },
            "en_us": {
                "title": "English",
                "content": [[
                    {
                        "tag": "text",
                        "text": "hello"
                    }
                ]]
            }
        });

        assert_eq!(
            parse_post_content(Some(&en_us_over_ja_jp)),
            "English\nhello"
        );

        let custom_locale = json!({
            "fr_fr": {
                "title": "Français",
                "content": [[
                    {
                        "tag": "text",
                        "text": "bonjour"
                    }
                ]]
            }
        });

        assert_eq!(
            parse_post_content(Some(&custom_locale)),
            "Français\nbonjour"
        );
    }

    #[test]
    fn unsupported_lark_message_type_preserves_metadata_and_content() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_image_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_image",
                    "chat_id": "oc_1",
                    "chat_type": "group",
                    "message_type": "image",
                    "content": "{\"image_key\":\"img_v2_1\"}"
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.message_id, "om_image");
        assert_eq!(message.message_type, "image");
        assert_eq!(message.text, "");
        assert_eq!(message.raw_content, "{\"image_key\":\"img_v2_1\"}");
        assert_eq!(
            message.content.as_ref().expect("content")["image_key"],
            "img_v2_1"
        );
        assert_eq!(message.raw["header"]["event_type"], "im.message.receive_v1");
    }

    #[test]
    fn malformed_lark_message_content_does_not_drop_receive_event() {
        let payload = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_bad_content_1",
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": "om_bad",
                    "chat_id": "oc_1",
                    "message_type": "text",
                    "content": "{not valid json"
                }
            }
        });

        let event = parse_lark_event_payload(payload.to_string().as_bytes()).expect("event");
        let ChannelEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.message_id, "om_bad");
        assert_eq!(message.chat_type, MessageChatType::Unknown);
        assert_eq!(message.message_type, "text");
        assert_eq!(message.text, "");
        assert_eq!(message.raw_content, "{not valid json");
        assert_eq!(message.content, None);
        assert_eq!(
            message.raw["event"]["message"]["content"],
            "{not valid json"
        );
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
