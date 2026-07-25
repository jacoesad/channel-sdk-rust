use crate::lark_openapi::{
    FileCreateRequest, FileType, ImageCreateRequest, OpenApiClient, OpenApiMultipartTransport,
};
use crate::{Error, Result};

use super::ResourceType;

/// In-memory media selected for upload through [`MediaUploader`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaUpload {
    Image {
        bytes: Vec<u8>,
    },
    File {
        file_name: String,
        bytes: Vec<u8>,
    },
    Audio {
        file_name: String,
        bytes: Vec<u8>,
        duration_ms: u64,
    },
    Video {
        file_name: String,
        bytes: Vec<u8>,
        duration_ms: u64,
    },
}

impl MediaUpload {
    pub fn image(bytes: impl Into<Vec<u8>>) -> Self {
        Self::Image {
            bytes: bytes.into(),
        }
    }

    pub fn file(file_name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self::File {
            file_name: file_name.into(),
            bytes: bytes.into(),
        }
    }

    pub fn audio(
        file_name: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
        duration_ms: u64,
    ) -> Self {
        Self::Audio {
            file_name: file_name.into(),
            bytes: bytes.into(),
            duration_ms,
        }
    }

    pub fn video(
        file_name: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
        duration_ms: u64,
    ) -> Self {
        Self::Video {
            file_name: file_name.into(),
            bytes: bytes.into(),
            duration_ms,
        }
    }
}

/// Resource key and metadata returned after a successful upload.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UploadedResource {
    Image {
        image_key: String,
    },
    File {
        resource_type: ResourceType,
        file_key: String,
        file_name: String,
        duration_ms: Option<u64>,
    },
}

impl UploadedResource {
    pub fn resource_type(&self) -> ResourceType {
        match self {
            Self::Image { .. } => ResourceType::Image,
            Self::File { resource_type, .. } => *resource_type,
        }
    }

    pub fn key(&self) -> &str {
        match self {
            Self::Image { image_key } => image_key,
            Self::File { file_key, .. } => file_key,
        }
    }
}

/// High-level uploader for in-memory Lark/Feishu media.
///
/// The uploader performs one OpenAPI request and does not retry uploads,
/// resolve paths, fetch URLs, or infer audio/video duration.
#[derive(Debug, Clone)]
pub struct MediaUploader<T> {
    client: OpenApiClient<T>,
}

impl<T> MediaUploader<T>
where
    T: OpenApiMultipartTransport,
{
    pub fn new(client: OpenApiClient<T>) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &OpenApiClient<T> {
        &self.client
    }

    pub async fn upload(&self, upload: MediaUpload) -> Result<UploadedResource> {
        match upload {
            MediaUpload::Image { bytes } => {
                let image_key = self
                    .client
                    .create_image(ImageCreateRequest::message(bytes))
                    .await?;
                Ok(UploadedResource::Image {
                    image_key: image_key.0,
                })
            }
            MediaUpload::File { file_name, bytes } => {
                self.upload_file(ResourceType::File, FileType::Stream, file_name, bytes, None)
                    .await
            }
            MediaUpload::Audio {
                file_name,
                bytes,
                duration_ms,
            } => {
                validate_duration(duration_ms)?;
                self.upload_file(
                    ResourceType::Audio,
                    FileType::Opus,
                    file_name,
                    bytes,
                    Some(duration_ms),
                )
                .await
            }
            MediaUpload::Video {
                file_name,
                bytes,
                duration_ms,
            } => {
                validate_duration(duration_ms)?;
                self.upload_file(
                    ResourceType::Media,
                    FileType::Mp4,
                    file_name,
                    bytes,
                    Some(duration_ms),
                )
                .await
            }
        }
    }

    async fn upload_file(
        &self,
        resource_type: ResourceType,
        file_type: FileType,
        file_name: String,
        bytes: Vec<u8>,
        duration_ms: Option<u64>,
    ) -> Result<UploadedResource> {
        let mut request = FileCreateRequest::new(file_type, file_name.clone(), bytes);
        if let Some(duration_ms) = duration_ms {
            request = request.duration_ms(duration_ms);
        }
        let file_key = self.client.create_file(request).await?;

        Ok(UploadedResource::File {
            resource_type,
            file_key: file_key.0,
            file_name,
            duration_ms,
        })
    }
}

fn validate_duration(duration_ms: u64) -> Result<()> {
    if duration_ms == 0 {
        return Err(Error::Validation(
            "audio/video duration_ms must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ChannelConfig;
    use crate::lark_openapi::test_support::{FakeTransport, block_on};
    use crate::lark_openapi::{HttpResponse, MultipartPart};

    fn uploader(response_data: serde_json::Value) -> (MediaUploader<FakeTransport>, FakeTransport) {
        let transport = FakeTransport::with_multipart_responses(
            vec![HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            )],
            vec![HttpResponse::json(
                200,
                json!({"code": 0, "msg": "ok", "data": response_data}),
            )],
        );
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        (MediaUploader::new(client), transport)
    }

    #[test]
    fn uploads_message_images() {
        let (uploader, transport) = uploader(json!({"image_key": "img_123"}));

        let resource =
            block_on(uploader.upload(MediaUpload::image(vec![1, 2]))).expect("uploaded image");

        assert_eq!(
            resource,
            UploadedResource::Image {
                image_key: "img_123".to_owned()
            }
        );
        assert_eq!(resource.resource_type(), ResourceType::Image);
        assert_eq!(resource.key(), "img_123");
        assert!(matches!(
            transport.multipart_calls()[0].parts.as_slice(),
            [
                MultipartPart::Text { name, value },
                MultipartPart::File { name: file_name, .. }
            ] if name == "image_type" && value == "message" && file_name == "image"
        ));
    }

    #[test]
    fn maps_file_audio_and_video_uploads() {
        let cases = [
            (
                MediaUpload::file("notes.txt", vec![1]),
                ResourceType::File,
                "stream",
                None,
            ),
            (
                MediaUpload::audio("voice.opus", vec![2], 1500),
                ResourceType::Audio,
                "opus",
                Some("1500"),
            ),
            (
                MediaUpload::video("clip.mp4", vec![3], 2500),
                ResourceType::Media,
                "mp4",
                Some("2500"),
            ),
        ];

        for (upload, resource_type, file_type, duration) in cases {
            let (uploader, transport) = uploader(json!({"file_key": "file_123"}));
            let resource = block_on(uploader.upload(upload)).expect("uploaded file");

            assert_eq!(resource.resource_type(), resource_type);
            assert_eq!(resource.key(), "file_123");
            let parts = &transport.multipart_calls()[0].parts;
            assert!(parts.contains(&MultipartPart::Text {
                name: "file_type".to_owned(),
                value: file_type.to_owned(),
            }));
            assert_eq!(
                parts.iter().find_map(|part| match part {
                    MultipartPart::Text { name, value } if name == "duration" => {
                        Some(value.as_str())
                    }
                    _ => None,
                }),
                duration
            );
        }
    }

    #[test]
    fn rejects_zero_audio_or_video_duration_before_authentication() {
        let transport = FakeTransport::with_multipart_responses(Vec::new(), Vec::new());
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let uploader = MediaUploader::new(client);

        let audio = block_on(uploader.upload(MediaUpload::audio("voice.opus", vec![1], 0)))
            .expect_err("zero audio duration");
        let video = block_on(uploader.upload(MediaUpload::video("clip.mp4", vec![1], 0)))
            .expect_err("zero video duration");

        assert!(matches!(audio, Error::Validation(_)));
        assert!(matches!(video, Error::Validation(_)));
        assert!(transport.calls().is_empty());
        assert!(transport.multipart_calls().is_empty());
    }
}
