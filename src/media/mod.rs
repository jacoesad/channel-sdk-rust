use std::fmt;

use serde::{Deserialize, Serialize};

use crate::debug::RedactedOption;

mod downloader;
mod uploader;

pub use downloader::MediaDownloader;
pub use uploader::{MediaUpload, MediaUploader, UploadedResource};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Image,
    File,
    Folder,
    Audio,
    Media,
    Sticker,
    #[default]
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDescriptor {
    pub message_id: String,
    #[serde(default)]
    pub resource_type: ResourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl fmt::Debug for ResourceDescriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResourceDescriptor")
            .field("message_id", &self.message_id)
            .field("resource_type", &self.resource_type)
            .field("file_key", &RedactedOption(&self.file_key))
            .field("image_key", &RedactedOption(&self.image_key))
            .field(
                "file_name_chars",
                &self
                    .file_name
                    .as_ref()
                    .map(|file_name| file_name.chars().count()),
            )
            .field("duration_ms", &self.duration_ms)
            .finish()
    }
}

/// In-memory resource downloaded from a Lark/Feishu message.
#[derive(Clone, PartialEq, Eq)]
pub struct DownloadedResource {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
}

impl fmt::Debug for DownloadedResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DownloadedResource")
            .field("bytes_len", &self.bytes.len())
            .field("content_type", &self.content_type)
            .field(
                "has_content_disposition",
                &self.content_disposition.is_some(),
            )
            .finish()
    }
}

impl DownloadedResource {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_summarizes_downloaded_bytes() {
        let resource = DownloadedResource {
            bytes: vec![1, 2, 3, 4],
            content_type: Some("application/octet-stream".to_owned()),
            content_disposition: Some("attachment; filename=private-report.pdf".to_owned()),
        };

        let debug = format!("{resource:?}");
        assert!(debug.contains("bytes_len: 4"));
        assert!(debug.contains("application/octet-stream"));
        assert!(debug.contains("has_content_disposition: true"));
        assert!(!debug.contains("[1, 2, 3, 4]"));
        assert!(!debug.contains("private-report.pdf"));
    }

    #[test]
    fn debug_redacts_resource_keys_and_summarizes_file_names() {
        let descriptor = ResourceDescriptor {
            message_id: "om_123".to_owned(),
            resource_type: ResourceType::Media,
            file_key: Some("file-secret".to_owned()),
            image_key: Some("image-secret".to_owned()),
            file_name: Some("private-video.mp4".to_owned()),
            duration_ms: Some(1200),
        };

        let debug = format!("{descriptor:?}");
        assert!(debug.contains("message_id: \"om_123\""));
        assert!(debug.contains("file_name_chars: Some(17)"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("file-secret"));
        assert!(!debug.contains("image-secret"));
        assert!(!debug.contains("private-video.mp4"));
    }
}
