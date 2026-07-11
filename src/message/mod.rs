use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::media::ResourceDescriptor;

mod post;
mod sender;

pub use post::{PostContent, PostContentBuilder, PostDocument, PostElement, PostStyle};
pub use sender::{MessageBuilder, MessageReplyBuilder, MessageSender, MessageSenderOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Recipient {
    Chat(String),
    User(String),
}

/// Outbound message content supported by the Channel SDK.
///
/// This enum is non-exhaustive because future releases may add content types.
/// Its serde representation may add matching variants as well, so older readers
/// are not guaranteed to deserialize data written by newer releases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum MessageContent {
    Text { text: String },
    Post { post: PostContent },
    Card { card: Value },
    Custom { msg_type: String, content: Value },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageChatType {
    P2p,
    Group,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSenderType {
    User,
    Bot,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSenderInfo {
    #[serde(default)]
    pub open_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub union_id: Option<String>,
    #[serde(default)]
    pub sender_type: MessageSenderType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMention {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub open_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub union_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub mentioned_type: MessageSenderType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedMessage {
    pub message_id: String,
    pub chat_id: String,
    #[serde(default)]
    pub chat_type: MessageChatType,
    pub sender_id: String,
    #[serde(default)]
    pub sender: MessageSenderInfo,
    #[serde(default)]
    pub message_type: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_mentions")]
    pub mentions: Vec<MessageMention>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceDescriptor>,
    #[serde(default)]
    pub raw: Value,
}

impl NormalizedMessage {
    pub fn mentions_bot(&self, bot_open_id: &str) -> bool {
        if bot_open_id.is_empty() {
            return false;
        }

        self.mentions
            .iter()
            .any(|mention| mention.open_id == bot_open_id)
    }
}

fn deserialize_mentions<'de, D>(deserializer: D) -> Result<Vec<MessageMention>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MentionValue {
        Structured(MessageMention),
        LegacyOpenId(String),
    }

    Vec::<MentionValue>::deserialize(deserializer).map(|mentions| {
        mentions
            .into_iter()
            .map(|mention| match mention {
                MentionValue::Structured(mention) => mention,
                MentionValue::LegacyOpenId(open_id) => MessageMention {
                    key: String::new(),
                    open_id,
                    user_id: None,
                    union_id: None,
                    name: None,
                    mentioned_type: MessageSenderType::Unknown,
                },
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn deserializes_older_normalized_message_json_with_defaults() {
        let value = json!({
            "message_id": "om_1",
            "chat_id": "oc_1",
            "sender_id": "ou_sender",
            "text": "hello",
            "mentions": ["ou_bot"]
        });

        let message: NormalizedMessage = serde_json::from_value(value).expect("message");

        assert_eq!(message.chat_type, MessageChatType::Unknown);
        assert_eq!(message.sender, MessageSenderInfo::default());
        assert_eq!(message.message_type, "");
        assert_eq!(message.raw_content, "");
        assert_eq!(message.content, None);
        assert_eq!(message.mentions.len(), 1);
        assert!(message.resources.is_empty());
        assert_eq!(message.mentions[0].open_id, "ou_bot");
        assert_eq!(
            message.mentions[0].mentioned_type,
            MessageSenderType::Unknown
        );
        assert!(message.mentions_bot("ou_bot"));
    }

    #[test]
    fn serializes_absent_mention_name_without_null_field() {
        let mention = MessageMention {
            key: "@_user_1".to_owned(),
            open_id: "ou_bot".to_owned(),
            user_id: None,
            union_id: None,
            name: None,
            mentioned_type: MessageSenderType::Unknown,
        };

        let value = serde_json::to_value(mention).expect("mention");

        assert_eq!(value["open_id"], "ou_bot");
        assert!(value.get("user_id").is_none());
        assert!(value.get("union_id").is_none());
        assert!(value.get("name").is_none());
    }

    #[test]
    fn mentions_bot_ignores_empty_bot_open_id() {
        let message = NormalizedMessage {
            message_id: "om_1".to_owned(),
            chat_id: "oc_1".to_owned(),
            chat_type: MessageChatType::Group,
            sender_id: "ou_sender".to_owned(),
            sender: MessageSenderInfo::default(),
            message_type: "text".to_owned(),
            text: "hello".to_owned(),
            raw_content: String::new(),
            content: None,
            root_id: None,
            parent_id: None,
            thread_id: None,
            mentions: vec![MessageMention {
                key: String::new(),
                open_id: String::new(),
                user_id: None,
                union_id: None,
                name: None,
                mentioned_type: MessageSenderType::Unknown,
            }],
            resources: Vec::new(),
            raw: Value::Null,
        };

        assert!(!message.mentions_bot(""));
    }
}
