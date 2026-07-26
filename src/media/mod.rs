use std::fmt;

use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            .field("content_disposition", &self.content_disposition)
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
            content_disposition: None,
        };

        let debug = format!("{resource:?}");
        assert!(debug.contains("bytes_len: 4"));
        assert!(debug.contains("application/octet-stream"));
        assert!(!debug.contains("[1, 2, 3, 4]"));
    }
}
