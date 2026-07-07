use serde::{Deserialize, Serialize};

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
