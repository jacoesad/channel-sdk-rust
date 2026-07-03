use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

mod sender;

pub use sender::{MessageBuilder, MessageReplyBuilder, MessageSender, MessageSenderOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Recipient {
    Chat(String),
    User(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text { text: String },
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
    pub open_id: String,
    pub sender_type: MessageSenderType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMention {
    pub key: String,
    pub open_id: String,
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
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_mentions")]
    pub mentions: Vec<MessageMention>,
    #[serde(default)]
    pub raw: Value,
}

impl NormalizedMessage {
    pub fn mentions_bot(&self, bot_open_id: &str) -> bool {
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
        assert_eq!(message.mentions.len(), 1);
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
            name: None,
            mentioned_type: MessageSenderType::Unknown,
        };

        let value = serde_json::to_value(mention).expect("mention");

        assert_eq!(value["open_id"], "ou_bot");
        assert!(value.get("name").is_none());
    }
}
