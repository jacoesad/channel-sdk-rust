use crate::media::{ResourceType, UploadedResource};
use crate::{Error, MessageContent, Result};

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
    use super::*;

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

    fn uploaded_file(resource_type: ResourceType) -> UploadedResource {
        UploadedResource::File {
            resource_type,
            file_key: "file_123".to_owned(),
            file_name: "resource.bin".to_owned(),
            duration_ms: None,
        }
    }
}
