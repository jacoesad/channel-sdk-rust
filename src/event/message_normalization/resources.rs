use serde_json::Value;

use crate::media::{ResourceDescriptor, ResourceType};

use super::post::select_post_document;

pub(in crate::event) fn normalize_message_resources(
    message_id: &str,
    message_type: &str,
    content: Option<&Value>,
) -> Vec<ResourceDescriptor> {
    let Some(content) = content else {
        return Vec::new();
    };

    let mut resources = Vec::new();
    match message_type {
        "image" => push_resource(
            &mut resources,
            resource_descriptor(message_id, ResourceType::Image, None, image_key(content)),
        ),
        "file" => {
            let mut resource =
                resource_descriptor(message_id, ResourceType::File, file_key(content), None);
            resource.file_name = file_name(content);
            push_resource(&mut resources, resource);
        }
        "folder" => {
            let mut resource =
                resource_descriptor(message_id, ResourceType::Folder, file_key(content), None);
            resource.file_name = file_name(content);
            push_resource(&mut resources, resource);
        }
        "audio" => {
            let mut resource =
                resource_descriptor(message_id, ResourceType::Audio, file_key(content), None);
            resource.duration_ms = duration_ms(content);
            push_resource(&mut resources, resource);
        }
        "media" => push_resource(
            &mut resources,
            media_resource_descriptor(
                message_id,
                file_key(content),
                image_key(content),
                file_name(content),
                duration_ms(content),
            ),
        ),
        "sticker" => push_resource(
            &mut resources,
            resource_descriptor(message_id, ResourceType::Sticker, file_key(content), None),
        ),
        "post" => {
            for resource in parse_post_resources(message_id, content) {
                push_resource(&mut resources, resource);
            }
        }
        _ => {}
    }

    resources
}

fn parse_post_resources(message_id: &str, content: &Value) -> Vec<ResourceDescriptor> {
    let Some(document) = select_post_document(content) else {
        return Vec::new();
    };

    let mut resources = Vec::new();
    for content_key in ["content", "content_v2"] {
        let Some(lines) = document.get(content_key).and_then(Value::as_array) else {
            continue;
        };

        for line in lines {
            let Some(elements) = line.as_array() else {
                continue;
            };

            for element in elements {
                if let Some(resource) = post_resource(message_id, element) {
                    push_resource(&mut resources, resource);
                }
            }
        }
    }

    resources
}

fn post_resource(message_id: &str, element: &Value) -> Option<ResourceDescriptor> {
    match element.get("tag").and_then(Value::as_str) {
        Some("img") => Some(resource_descriptor(
            message_id,
            ResourceType::Image,
            None,
            image_key(element),
        )),
        Some("media") => Some(media_resource_descriptor(
            message_id,
            file_key(element),
            image_key(element),
            file_name(element),
            duration_ms(element),
        )),
        _ => None,
    }
}

fn resource_descriptor(
    message_id: &str,
    resource_type: ResourceType,
    file_key: Option<String>,
    image_key: Option<String>,
) -> ResourceDescriptor {
    ResourceDescriptor {
        message_id: message_id.to_owned(),
        resource_type,
        file_key,
        image_key,
        file_name: None,
        duration_ms: None,
    }
}

fn media_resource_descriptor(
    message_id: &str,
    file_key: Option<String>,
    image_key: Option<String>,
    file_name: Option<String>,
    duration_ms: Option<u64>,
) -> ResourceDescriptor {
    let mut resource = resource_descriptor(message_id, ResourceType::Media, file_key, image_key);
    resource.file_name = file_name;
    resource.duration_ms = duration_ms;
    resource
}

fn file_key(value: &Value) -> Option<String> {
    non_empty_string(value, "file_key")
}

fn image_key(value: &Value) -> Option<String> {
    non_empty_string(value, "image_key")
}

fn file_name(value: &Value) -> Option<String> {
    non_empty_string(value, "file_name")
}

fn non_empty_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn duration_ms(value: &Value) -> Option<u64> {
    value.get("duration").and_then(|duration| {
        duration
            .as_u64()
            .or_else(|| duration.as_str()?.parse().ok())
    })
}

fn push_resource(resources: &mut Vec<ResourceDescriptor>, resource: ResourceDescriptor) {
    if resource.file_key.is_none() && resource.image_key.is_none() {
        return;
    }

    if let Some(existing) = resources
        .iter_mut()
        .find(|existing| same_resource(existing, &resource))
    {
        merge_resource(existing, resource);
    } else {
        resources.push(resource);
    }
}

fn same_resource(left: &ResourceDescriptor, right: &ResourceDescriptor) -> bool {
    if left.resource_type != right.resource_type {
        return false;
    }

    match left.resource_type {
        ResourceType::Image => {
            same_resource_key(left.image_key.as_deref(), right.image_key.as_deref())
        }
        ResourceType::File | ResourceType::Folder | ResourceType::Audio | ResourceType::Sticker => {
            same_resource_key(left.file_key.as_deref(), right.file_key.as_deref())
        }
        ResourceType::Media => {
            same_resource_key(left.file_key.as_deref(), right.file_key.as_deref())
                || (left.file_key.is_none()
                    && right.file_key.is_none()
                    && same_resource_key(left.image_key.as_deref(), right.image_key.as_deref()))
        }
        ResourceType::Unknown => {
            same_resource_key(left.file_key.as_deref(), right.file_key.as_deref())
                || same_resource_key(left.image_key.as_deref(), right.image_key.as_deref())
        }
    }
}

fn same_resource_key(left: Option<&str>, right: Option<&str>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if !left.is_empty() && left == right)
}

fn merge_resource(existing: &mut ResourceDescriptor, next: ResourceDescriptor) {
    if existing.file_key.is_none() {
        existing.file_key = next.file_key;
    }
    if existing.image_key.is_none() {
        existing.image_key = next.image_key;
    }
    if existing.file_name.is_none() {
        existing.file_name = next.file_name;
    }
    if existing.duration_ms.is_none() {
        existing.duration_ms = next.duration_ms;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalizes_direct_media_message_resources() {
        let image = json!({
            "image_key": "img_1"
        });
        let resources = normalize_message_resources("om_image", "image", Some(&image));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].message_id, "om_image");
        assert_eq!(resources[0].resource_type, ResourceType::Image);
        assert_eq!(resources[0].file_key, None);
        assert_eq!(resources[0].image_key.as_deref(), Some("img_1"));

        let file = json!({
            "file_key": "file_1",
            "file_name": "report.pdf"
        });
        let resources = normalize_message_resources("om_file", "file", Some(&file));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].resource_type, ResourceType::File);
        assert_eq!(resources[0].file_key.as_deref(), Some("file_1"));
        assert_eq!(resources[0].file_name.as_deref(), Some("report.pdf"));

        let audio = json!({
            "file_key": "audio_1",
            "duration": 2000
        });
        let resources = normalize_message_resources("om_audio", "audio", Some(&audio));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].resource_type, ResourceType::Audio);
        assert_eq!(resources[0].file_key.as_deref(), Some("audio_1"));
        assert_eq!(resources[0].duration_ms, Some(2000));

        let media = json!({
            "file_key": "video_1",
            "image_key": "img_cover_1",
            "file_name": "demo.mp4",
            "duration": "3000"
        });
        let resources = normalize_message_resources("om_media", "media", Some(&media));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].resource_type, ResourceType::Media);
        assert_eq!(resources[0].file_key.as_deref(), Some("video_1"));
        assert_eq!(resources[0].image_key.as_deref(), Some("img_cover_1"));
        assert_eq!(resources[0].file_name.as_deref(), Some("demo.mp4"));
        assert_eq!(resources[0].duration_ms, Some(3000));
    }

    #[test]
    fn normalizes_folder_and_sticker_resource_descriptors() {
        let folder = json!({
            "file_key": "folder_1",
            "file_name": "folder"
        });
        let resources = normalize_message_resources("om_folder", "folder", Some(&folder));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].resource_type, ResourceType::Folder);
        assert_eq!(resources[0].file_key.as_deref(), Some("folder_1"));
        assert_eq!(resources[0].file_name.as_deref(), Some("folder"));

        let sticker = json!({
            "file_key": "sticker_1"
        });
        let resources = normalize_message_resources("om_sticker", "sticker", Some(&sticker));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].resource_type, ResourceType::Sticker);
        assert_eq!(resources[0].file_key.as_deref(), Some("sticker_1"));
    }

    #[test]
    fn normalizes_post_media_resources_and_deduplicates_content_v2() {
        let content = json!({
            "title": "resources",
            "content": [
                [
                    {
                        "tag": "img",
                        "image_key": "img_1"
                    },
                    {
                        "tag": "media",
                        "file_key": "video_1"
                    }
                ]
            ],
            "content_v2": [
                [
                    {
                        "tag": "img",
                        "image_key": "img_1"
                    },
                    {
                        "tag": "media",
                        "file_key": "video_1",
                        "image_key": "img_cover_1",
                        "file_name": "demo.mp4",
                        "duration": 3000
                    }
                ]
            ]
        });

        let resources = normalize_message_resources("om_post", "post", Some(&content));
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].resource_type, ResourceType::Image);
        assert_eq!(resources[0].image_key.as_deref(), Some("img_1"));
        assert_eq!(resources[1].resource_type, ResourceType::Media);
        assert_eq!(resources[1].file_key.as_deref(), Some("video_1"));
        assert_eq!(resources[1].image_key.as_deref(), Some("img_cover_1"));
        assert_eq!(resources[1].file_name.as_deref(), Some("demo.mp4"));
        assert_eq!(resources[1].duration_ms, Some(3000));
    }

    #[test]
    fn normalizes_content_v2_only_post_media_resources() {
        let content = json!({
            "content_v2": [
                [
                    {
                        "tag": "media",
                        "file_key": "video_1",
                        "image_key": "img_cover_1",
                        "file_name": "demo.mp4",
                        "duration": 3000
                    }
                ]
            ]
        });

        let resources = normalize_message_resources("om_post", "post", Some(&content));
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].resource_type, ResourceType::Media);
        assert_eq!(resources[0].file_key.as_deref(), Some("video_1"));
        assert_eq!(resources[0].image_key.as_deref(), Some("img_cover_1"));
        assert_eq!(resources[0].file_name.as_deref(), Some("demo.mp4"));
        assert_eq!(resources[0].duration_ms, Some(3000));
    }

    #[test]
    fn skips_resource_descriptors_without_resource_keys() {
        let image = json!({
            "image_key": ""
        });
        assert!(normalize_message_resources("om_image", "image", Some(&image)).is_empty());

        let audio = json!({
            "duration": 2000
        });
        assert!(normalize_message_resources("om_audio", "audio", Some(&audio)).is_empty());
    }
}
