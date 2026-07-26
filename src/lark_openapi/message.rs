use serde::Serialize;
use serde_json::Value;

use crate::message::{MessageContent, MessageId, Recipient};
use crate::validation::validate_path_identifier;
use crate::{Error, Result};

use super::{OpenApiClient, OpenApiTransport};

const MESSAGE_PATH: &str = "/open-apis/im/v1/messages";

impl<T> OpenApiClient<T>
where
    T: OpenApiTransport,
{
    pub async fn create_message(
        &self,
        recipient: Recipient,
        content: MessageContent,
    ) -> Result<MessageId> {
        self.create_message_with_options(recipient, content, MessageCreateOptions::default())
            .await
    }

    pub async fn create_message_with_options(
        &self,
        recipient: Recipient,
        content: MessageContent,
        options: MessageCreateOptions,
    ) -> Result<MessageId> {
        let recipient = CreateMessageRecipient::try_from(recipient)?;
        let content = OpenApiMessageContent::try_from(content)?;
        let path = format!(
            "{}?receive_id_type={}",
            MESSAGE_PATH, recipient.receive_id_type
        );
        let request = CreateMessageRequest {
            receive_id: recipient.receive_id,
            msg_type: content.msg_type,
            content: serde_json::to_string(&content.content)?,
            uuid: options.uuid,
        };
        let response: MessageResponse = self.post_tenant_json(&path, &request).await?;

        Ok(MessageId(response.data.message_id))
    }

    pub async fn reply_message(
        &self,
        parent_message_id: MessageId,
        content: MessageContent,
    ) -> Result<MessageId> {
        self.reply_message_with_options(parent_message_id, content, MessageReplyOptions::default())
            .await
    }

    pub async fn reply_message_with_options(
        &self,
        parent_message_id: MessageId,
        content: MessageContent,
        options: MessageReplyOptions,
    ) -> Result<MessageId> {
        validate_message_id(&parent_message_id)?;
        let content = OpenApiMessageContent::try_from(content)?;
        let path = format!("{MESSAGE_PATH}/{}/reply", parent_message_id.0);
        let request = ReplyMessageRequest {
            msg_type: content.msg_type,
            content: serde_json::to_string(&content.content)?,
            uuid: options.uuid,
            reply_in_thread: options.reply_in_thread,
        };
        let response: MessageResponse = self.post_tenant_json(&path, &request).await?;

        Ok(MessageId(response.data.message_id))
    }
}

pub(super) fn validate_message_id(message_id: &MessageId) -> Result<()> {
    validate_path_identifier(&message_id.0, "message_id")
}

/// Optional parameters for creating a new message.
///
/// `uuid` is a caller-provided idempotency key forwarded to Lark/Feishu.
/// Reuse the same value when retrying the same logical message, and use a
/// new value when sending different content.
///
/// `OpenApiClient` does not generate this value automatically. Higher-level
/// retry helpers should create one key per logical send and reuse it for all
/// retry attempts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageCreateOptions {
    /// Request de-duplication key accepted by the Lark/Feishu create-message API.
    pub uuid: Option<String>,
}

impl MessageCreateOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_uuid(uuid: impl Into<String>) -> Self {
        Self {
            uuid: Some(uuid.into()),
        }
    }

    pub fn uuid(mut self, uuid: impl Into<String>) -> Self {
        self.uuid = Some(uuid.into());
        self
    }
}

/// Optional parameters for replying to an existing message.
///
/// `uuid` is a caller-provided idempotency key forwarded to Lark/Feishu.
/// Reuse the same value when retrying the same logical reply, and use a new
/// value when replying with different content.
///
/// `OpenApiClient` does not generate this value automatically. Higher-level
/// retry helpers should create one key per logical reply and reuse it for all
/// retry attempts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageReplyOptions {
    /// Request de-duplication key accepted by the Lark/Feishu reply-message API.
    pub uuid: Option<String>,
    /// Request that the reply is placed in a thread when the target chat supports it.
    pub reply_in_thread: Option<bool>,
}

impl MessageReplyOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_uuid(uuid: impl Into<String>) -> Self {
        Self {
            uuid: Some(uuid.into()),
            reply_in_thread: None,
        }
    }

    pub fn uuid(mut self, uuid: impl Into<String>) -> Self {
        self.uuid = Some(uuid.into());
        self
    }

    pub fn reply_in_thread(mut self, reply_in_thread: bool) -> Self {
        self.reply_in_thread = Some(reply_in_thread);
        self
    }
}

#[derive(Debug)]
struct CreateMessageRecipient {
    receive_id_type: &'static str,
    receive_id: String,
}

impl TryFrom<Recipient> for CreateMessageRecipient {
    type Error = Error;

    fn try_from(recipient: Recipient) -> Result<Self> {
        match recipient {
            Recipient::Chat(receive_id) => Ok(Self {
                receive_id_type: "chat_id",
                receive_id,
            }),
            Recipient::User(receive_id) => Ok(Self {
                receive_id_type: "open_id",
                receive_id,
            }),
        }
    }
}

struct OpenApiMessageContent {
    msg_type: String,
    content: Value,
}

impl TryFrom<MessageContent> for OpenApiMessageContent {
    type Error = Error;

    fn try_from(content: MessageContent) -> Result<Self> {
        match content {
            MessageContent::Text { text } => Ok(Self {
                msg_type: "text".to_owned(),
                content: serde_json::json!({ "text": text }),
            }),
            MessageContent::Post { post } => {
                post.validate()?;
                Ok(Self {
                    msg_type: "post".to_owned(),
                    content: serde_json::to_value(post)?,
                })
            }
            MessageContent::Image { image_key } => Ok(Self {
                msg_type: "image".to_owned(),
                content: serde_json::json!({
                    "image_key": validate_resource_key(image_key, "image_key")?
                }),
            }),
            MessageContent::File { file_key } => Ok(Self {
                msg_type: "file".to_owned(),
                content: serde_json::json!({
                    "file_key": validate_resource_key(file_key, "file_key")?
                }),
            }),
            MessageContent::Audio { file_key } => Ok(Self {
                msg_type: "audio".to_owned(),
                content: serde_json::json!({
                    "file_key": validate_resource_key(file_key, "file_key")?
                }),
            }),
            MessageContent::Media {
                file_key,
                image_key,
            } => {
                let mut content = serde_json::json!({
                    "file_key": validate_resource_key(file_key, "file_key")?
                });
                if let Some(image_key) = image_key {
                    content["image_key"] =
                        Value::String(validate_resource_key(image_key, "image_key")?);
                }
                Ok(Self {
                    msg_type: "media".to_owned(),
                    content,
                })
            }
            MessageContent::Card { card } => Ok(Self {
                msg_type: "interactive".to_owned(),
                content: card,
            }),
            MessageContent::CardReference { card_id } => {
                card_id.validate()?;
                Ok(Self {
                    msg_type: "interactive".to_owned(),
                    content: serde_json::json!({
                        "type": "card",
                        "data": { "card_id": card_id.as_str() }
                    }),
                })
            }
            MessageContent::Custom { msg_type, content } => Ok(Self { msg_type, content }),
        }
    }
}

fn validate_resource_key(key: String, field: &str) -> Result<String> {
    if key.trim().is_empty() {
        return Err(Error::Validation(format!("{field} must not be empty")));
    }
    Ok(key)
}

#[derive(Serialize)]
struct CreateMessageRequest {
    receive_id: String,
    msg_type: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uuid: Option<String>,
}

#[derive(Serialize)]
struct ReplyMessageRequest {
    msg_type: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_in_thread: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct MessageResponse {
    data: SendMessageData,
}

#[derive(Debug, serde::Deserialize)]
struct SendMessageData {
    message_id: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ChannelConfig;
    use crate::lark_openapi::HttpResponse;
    use crate::lark_openapi::test_support::{FakeTransport, block_on};

    fn message_client() -> (OpenApiClient<FakeTransport>, FakeTransport) {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": { "message_id": "om_media" }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        (client, transport)
    }

    #[test]
    fn create_message_serializes_official_media_content_shapes() {
        let cases = [
            (
                MessageContent::Image {
                    image_key: "img_123".to_owned(),
                },
                "image",
                json!({ "image_key": "img_123" }),
            ),
            (
                MessageContent::File {
                    file_key: "file_123".to_owned(),
                },
                "file",
                json!({ "file_key": "file_123" }),
            ),
            (
                MessageContent::Audio {
                    file_key: "file_audio".to_owned(),
                },
                "audio",
                json!({ "file_key": "file_audio" }),
            ),
            (
                MessageContent::Media {
                    file_key: "file_video".to_owned(),
                    image_key: Some("img_cover".to_owned()),
                },
                "media",
                json!({
                    "file_key": "file_video",
                    "image_key": "img_cover"
                }),
            ),
            (
                MessageContent::Media {
                    file_key: "file_video".to_owned(),
                    image_key: None,
                },
                "media",
                json!({ "file_key": "file_video" }),
            ),
        ];

        for (content, expected_type, expected_content) in cases {
            let (client, transport) = message_client();

            block_on(client.create_message(Recipient::Chat("oc_123".to_owned()), content))
                .expect("sent media message");

            let body = &transport.calls()[1].body;
            assert_eq!(body["msg_type"], expected_type);
            assert_eq!(
                serde_json::from_str::<Value>(body["content"].as_str().expect("content string"))
                    .expect("content json"),
                expected_content
            );
        }
    }

    #[test]
    fn media_content_rejects_empty_keys_before_authentication() {
        let cases = [
            MessageContent::Image {
                image_key: " ".to_owned(),
            },
            MessageContent::File {
                file_key: String::new(),
            },
            MessageContent::Audio {
                file_key: "\t".to_owned(),
            },
            MessageContent::Media {
                file_key: "file_video".to_owned(),
                image_key: Some(String::new()),
            },
        ];

        for content in cases {
            let transport = FakeTransport::new(Vec::new());
            let client =
                OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

            let error =
                block_on(client.create_message(Recipient::Chat("oc_123".to_owned()), content))
                    .expect_err("empty resource key");

            assert!(matches!(error, Error::Validation(_)));
            assert!(transport.calls().is_empty());
        }
    }
}
