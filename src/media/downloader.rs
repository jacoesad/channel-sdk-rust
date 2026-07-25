use crate::lark_openapi::{MessageResourceType, OpenApiBinaryTransport, OpenApiClient};
use crate::{DownloadedResource, Error, ResourceDescriptor, ResourceType, Result};

/// High-level downloader for resource descriptors produced by message
/// normalization.
#[derive(Debug, Clone)]
pub struct MediaDownloader<T> {
    client: OpenApiClient<T>,
}

impl<T> MediaDownloader<T>
where
    T: OpenApiBinaryTransport,
{
    pub fn new(client: OpenApiClient<T>) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &OpenApiClient<T> {
        &self.client
    }

    /// Downloads the primary resource represented by a normalized descriptor.
    ///
    /// For video/media descriptors this downloads the `file_key` payload, not
    /// the optional cover image. Call the low-level
    /// `OpenApiClient::get_message_resource` method with `Image` to download a
    /// cover explicitly.
    pub async fn download(&self, descriptor: &ResourceDescriptor) -> Result<DownloadedResource> {
        let (file_key, resource_type) = download_target(descriptor)?;
        self.client
            .get_message_resource(&descriptor.message_id, file_key, resource_type)
            .await
    }
}

fn download_target(descriptor: &ResourceDescriptor) -> Result<(&str, MessageResourceType)> {
    match descriptor.resource_type {
        ResourceType::Image => descriptor
            .image_key
            .as_deref()
            .map(|file_key| (file_key, MessageResourceType::Image))
            .ok_or_else(|| missing_resource_key("image_key")),
        ResourceType::File | ResourceType::Audio | ResourceType::Media => descriptor
            .file_key
            .as_deref()
            .map(|file_key| (file_key, MessageResourceType::File))
            .ok_or_else(|| missing_resource_key("file_key")),
        ResourceType::Folder => Err(unsupported_resource_type("folder")),
        ResourceType::Sticker => Err(unsupported_resource_type("sticker")),
        ResourceType::Unknown => Err(unsupported_resource_type("unknown")),
    }
}

fn missing_resource_key(name: &str) -> Error {
    Error::Validation(format!("resource descriptor is missing {name}"))
}

fn unsupported_resource_type(name: &str) -> Error {
    Error::Validation(format!(
        "{name} resources are not supported by the message resource download API"
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::ChannelConfig;
    use crate::lark_openapi::test_support::{FakeTransport, block_on};
    use crate::lark_openapi::{BinaryHttpResponse, HttpResponse};

    fn downloader(bytes: &[u8]) -> (MediaDownloader<FakeTransport>, FakeTransport) {
        let transport = FakeTransport::with_binary_responses(
            vec![HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            )],
            vec![BinaryHttpResponse::new(
                200,
                BTreeMap::new(),
                bytes.to_vec(),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        (MediaDownloader::new(client), transport)
    }

    fn descriptor(resource_type: ResourceType) -> ResourceDescriptor {
        ResourceDescriptor {
            message_id: "om_123".to_owned(),
            resource_type,
            file_key: None,
            image_key: None,
            file_name: None,
            duration_ms: None,
        }
    }

    #[test]
    fn downloads_image_descriptors_by_image_key() {
        let (downloader, transport) = downloader(b"image");
        let resource = ResourceDescriptor {
            image_key: Some("img_456".to_owned()),
            ..descriptor(ResourceType::Image)
        };

        let downloaded = block_on(downloader.download(&resource)).expect("downloaded image");

        assert_eq!(downloaded.bytes, b"image");
        assert_eq!(
            transport.calls()[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_123/resources/img_456?type=image"
        );
    }

    #[test]
    fn downloads_file_audio_and_media_descriptors_as_files() {
        for resource_type in [ResourceType::File, ResourceType::Audio, ResourceType::Media] {
            let (downloader, transport) = downloader(b"media");
            let resource = ResourceDescriptor {
                file_key: Some("file_456".to_owned()),
                image_key: Some("img_cover".to_owned()),
                ..descriptor(resource_type)
            };

            block_on(downloader.download(&resource)).expect("downloaded media");

            assert!(
                transport.calls()[1]
                    .url
                    .as_str()
                    .ends_with("/resources/file_456?type=file")
            );
        }
    }

    #[test]
    fn rejects_unsupported_or_incomplete_descriptors_before_authentication() {
        for resource in [
            descriptor(ResourceType::Folder),
            descriptor(ResourceType::Sticker),
            descriptor(ResourceType::Unknown),
            descriptor(ResourceType::Image),
            descriptor(ResourceType::File),
        ] {
            let transport = FakeTransport::with_binary_responses(Vec::new(), Vec::new());
            let client =
                OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
            let downloader = MediaDownloader::new(client);

            let error = block_on(downloader.download(&resource)).expect_err("invalid descriptor");

            assert!(matches!(error, Error::Validation(_)));
            assert!(transport.calls().is_empty());
        }
    }
}
