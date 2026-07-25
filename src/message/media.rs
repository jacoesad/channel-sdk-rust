use crate::lark_openapi::OpenApiTransport;
use crate::media::{ResourceType, UploadedResource};
use crate::{
    Error, MessageBuilder, MessageContent, MessageId, MessageReplyBuilder, MessageSender,
    Recipient, Result,
};

impl<T> MessageSender<T>
where
    T: OpenApiTransport,
{
    /// Starts building an image message from an uploaded image key.
    pub fn image_message(
        &self,
        recipient: Recipient,
        image_key: impl Into<String>,
    ) -> MessageBuilder<'_, T> {
        self.message(
            recipient,
            MessageContent::Image {
                image_key: image_key.into(),
            },
        )
    }

    /// Starts building a file message from an uploaded file key.
    pub fn file_message(
        &self,
        recipient: Recipient,
        file_key: impl Into<String>,
    ) -> MessageBuilder<'_, T> {
        self.message(
            recipient,
            MessageContent::File {
                file_key: file_key.into(),
            },
        )
    }

    /// Starts building an audio message from an uploaded OPUS file key.
    pub fn audio_message(
        &self,
        recipient: Recipient,
        file_key: impl Into<String>,
    ) -> MessageBuilder<'_, T> {
        self.message(
            recipient,
            MessageContent::Audio {
                file_key: file_key.into(),
            },
        )
    }

    /// Starts building a video `media` message from an uploaded MP4 file key.
    pub fn media_message(
        &self,
        recipient: Recipient,
        file_key: impl Into<String>,
        image_key: Option<String>,
    ) -> MessageBuilder<'_, T> {
        self.message(
            recipient,
            MessageContent::Media {
                file_key: file_key.into(),
                image_key,
            },
        )
    }

    /// Starts building an image reply from an uploaded image key.
    pub fn image_reply(
        &self,
        parent_message_id: MessageId,
        image_key: impl Into<String>,
    ) -> MessageReplyBuilder<'_, T> {
        self.reply(
            parent_message_id,
            MessageContent::Image {
                image_key: image_key.into(),
            },
        )
    }

    /// Starts building a file reply from an uploaded file key.
    pub fn file_reply(
        &self,
        parent_message_id: MessageId,
        file_key: impl Into<String>,
    ) -> MessageReplyBuilder<'_, T> {
        self.reply(
            parent_message_id,
            MessageContent::File {
                file_key: file_key.into(),
            },
        )
    }

    /// Starts building an audio reply from an uploaded OPUS file key.
    pub fn audio_reply(
        &self,
        parent_message_id: MessageId,
        file_key: impl Into<String>,
    ) -> MessageReplyBuilder<'_, T> {
        self.reply(
            parent_message_id,
            MessageContent::Audio {
                file_key: file_key.into(),
            },
        )
    }

    /// Starts building a video `media` reply from an uploaded MP4 file key.
    pub fn media_reply(
        &self,
        parent_message_id: MessageId,
        file_key: impl Into<String>,
        image_key: Option<String>,
    ) -> MessageReplyBuilder<'_, T> {
        self.reply(
            parent_message_id,
            MessageContent::Media {
                file_key: file_key.into(),
                image_key,
            },
        )
    }
}

impl TryFrom<UploadedResource> for MessageContent {
    type Error = Error;

    fn try_from(resource: UploadedResource) -> Result<Self> {
        match resource {
            UploadedResource::Image { image_key } => Ok(Self::Image { image_key }),
            UploadedResource::File {
                resource_type,
                file_key,
                ..
            } => file_message_content(resource_type, file_key),
        }
    }
}

impl TryFrom<&UploadedResource> for MessageContent {
    type Error = Error;

    fn try_from(resource: &UploadedResource) -> Result<Self> {
        match resource {
            UploadedResource::Image { image_key } => Ok(Self::Image {
                image_key: image_key.clone(),
            }),
            UploadedResource::File {
                resource_type,
                file_key,
                ..
            } => file_message_content(*resource_type, file_key.clone()),
        }
    }
}

fn file_message_content(resource_type: ResourceType, file_key: String) -> Result<MessageContent> {
    match resource_type {
        ResourceType::File => Ok(MessageContent::File { file_key }),
        ResourceType::Audio => Ok(MessageContent::Audio { file_key }),
        ResourceType::Media => Ok(MessageContent::Media {
            file_key,
            image_key: None,
        }),
        unsupported => Err(Error::Validation(format!(
            "uploaded {unsupported:?} resource cannot be sent as a media message"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::ChannelConfig;
    use crate::lark_openapi::test_support::{FakeCall, FakeTransport, block_on};
    use crate::lark_openapi::{HttpResponse, OpenApiClient};

    #[test]
    fn maps_uploaded_resources_to_message_content() {
        let cases = [
            (
                UploadedResource::Image {
                    image_key: "img_123".to_owned(),
                },
                MessageContent::Image {
                    image_key: "img_123".to_owned(),
                },
            ),
            (
                uploaded_file(ResourceType::File),
                MessageContent::File {
                    file_key: "file_123".to_owned(),
                },
            ),
            (
                uploaded_file(ResourceType::Audio),
                MessageContent::Audio {
                    file_key: "file_123".to_owned(),
                },
            ),
            (
                uploaded_file(ResourceType::Media),
                MessageContent::Media {
                    file_key: "file_123".to_owned(),
                    image_key: None,
                },
            ),
        ];

        for (uploaded, expected) in cases {
            assert_eq!(
                MessageContent::try_from(&uploaded).expect("borrowed conversion"),
                expected
            );
            assert_eq!(
                MessageContent::try_from(uploaded).expect("owned conversion"),
                expected
            );
        }
    }

    #[test]
    fn rejects_unsupported_uploaded_resource_types() {
        let error = MessageContent::try_from(uploaded_file(ResourceType::Folder))
            .expect_err("unsupported resource type");

        assert!(matches!(
            error,
            Error::Validation(message)
                if message == "uploaded Folder resource cannot be sent as a media message"
        ));
    }

    #[test]
    fn media_message_helpers_use_official_content_types() {
        let (sender, transport) = sender(4);
        let recipient = Recipient::Chat("oc_123".to_owned());

        block_on(sender.image_message(recipient.clone(), "img_123").send()).expect("sent image");
        block_on(sender.file_message(recipient.clone(), "file_123").send()).expect("sent file");
        block_on(sender.audio_message(recipient.clone(), "file_audio").send()).expect("sent audio");
        block_on(
            sender
                .media_message(recipient, "file_video", Some("img_cover".to_owned()))
                .send(),
        )
        .expect("sent media");

        assert_media_calls(
            &transport.calls()[1..],
            &[
                ("image", json!({ "image_key": "img_123" })),
                ("file", json!({ "file_key": "file_123" })),
                ("audio", json!({ "file_key": "file_audio" })),
                (
                    "media",
                    json!({ "file_key": "file_video", "image_key": "img_cover" }),
                ),
            ],
        );
    }

    #[test]
    fn media_reply_helpers_use_official_content_types() {
        let (sender, transport) = sender(4);
        let parent = MessageId("om_parent".to_owned());

        block_on(sender.image_reply(parent.clone(), "img_123").send()).expect("replied image");
        block_on(sender.file_reply(parent.clone(), "file_123").send()).expect("replied file");
        block_on(sender.audio_reply(parent.clone(), "file_audio").send()).expect("replied audio");
        block_on(
            sender
                .media_reply(parent, "file_video", None)
                .reply_in_thread(true)
                .send(),
        )
        .expect("replied media");

        let calls = transport.calls();
        assert_media_calls(
            &calls[1..],
            &[
                ("image", json!({ "image_key": "img_123" })),
                ("file", json!({ "file_key": "file_123" })),
                ("audio", json!({ "file_key": "file_audio" })),
                ("media", json!({ "file_key": "file_video" })),
            ],
        );
        assert!(calls[1..].iter().all(|call| {
            call.url.as_str() == "https://open.feishu.cn/open-apis/im/v1/messages/om_parent/reply"
        }));
        assert_eq!(calls[4].body["reply_in_thread"], true);
    }

    fn sender(message_count: usize) -> (MessageSender<FakeTransport>, FakeTransport) {
        let mut responses = vec![HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        )];
        responses.extend((0..message_count).map(|index| {
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": { "message_id": format!("om_{index}") }
                }),
            )
        }));
        let transport = FakeTransport::new(responses);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        (MessageSender::new(client), transport)
    }

    fn assert_media_calls(calls: &[FakeCall], expected: &[(&str, Value)]) {
        for (call, (expected_type, expected_content)) in calls.iter().zip(expected) {
            assert_eq!(call.body["msg_type"], *expected_type);
            let content = serde_json::from_str::<Value>(
                call.body["content"].as_str().expect("content string"),
            )
            .expect("content json");
            assert_eq!(&content, expected_content);
        }
    }

    fn uploaded_file(resource_type: ResourceType) -> UploadedResource {
        UploadedResource::File {
            resource_type,
            file_key: "file_123".to_owned(),
            file_name: "resource.bin".to_owned(),
            duration_ms: None,
        }
    }
}
