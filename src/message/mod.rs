use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageChatType {
    P2p,
    Group,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSenderType {
    User,
    Bot,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSenderInfo {
    pub open_id: String,
    pub sender_type: MessageSenderType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMention {
    pub key: String,
    pub open_id: String,
    pub name: Option<String>,
    pub mentioned_type: MessageSenderType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedMessage {
    pub message_id: String,
    pub chat_id: String,
    pub chat_type: MessageChatType,
    pub sender_id: String,
    pub sender: MessageSenderInfo,
    pub message_type: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub mentions: Vec<MessageMention>,
    #[serde(default)]
    pub raw: Value,
}

impl NormalizedMessage {
    pub fn mentions_bot(&self, bot_open_id: &str) -> bool {
        self.mentions.iter().any(|mention| {
            mention.mentioned_type == MessageSenderType::Bot && mention.open_id == bot_open_id
        })
    }
}
