use serde_json::Value;

use crate::media::{ResourceDescriptor, ResourceType};
use crate::message::{MessageMention, MessageSenderType};

pub(super) fn normalize_message_mentions(
    message_type: &str,
    content: Option<&Value>,
    event_mentions: Vec<MessageMention>,
) -> Vec<MessageMention> {
    let mut mentions = Vec::new();

    for mention in event_mentions {
        push_message_mention(&mut mentions, mention);
    }

    if message_type == "post" {
        for mention in parse_post_mentions(content) {
            push_message_mention(&mut mentions, mention);
        }
    }

    mentions
}

pub(super) fn normalize_message_text(
    message_type: &str,
    content: Option<&Value>,
    mentions: &[MessageMention],
) -> String {
    let text = match message_type {
        "text" => parse_text_content(content),
        "post" => parse_post_content(content),
        _ => String::new(),
    };

    resolve_mention_keys(text, mentions)
}

pub(super) fn normalize_message_resources(
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

fn parse_text_content(content: Option<&Value>) -> String {
    content
        .and_then(|value| value.get("text").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

pub(super) fn parse_post_content(content: Option<&Value>) -> String {
    content
        .and_then(select_post_document)
        .map(post_document_text)
        .unwrap_or_default()
}

fn parse_post_mentions(content: Option<&Value>) -> Vec<MessageMention> {
    let Some(document) = content.and_then(select_post_document) else {
        return Vec::new();
    };

    let Some(content) = post_text_content(document) else {
        return Vec::new();
    };

    let mut mentions = Vec::new();
    for line in content {
        let Some(elements) = line.as_array() else {
            continue;
        };
        for element in elements {
            if let Some(mention) = post_at_mention(element) {
                push_message_mention(&mut mentions, mention);
            }
        }
    }

    mentions
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

fn select_post_document(value: &Value) -> Option<&Value> {
    if has_legacy_post_content(value) {
        return Some(value);
    }

    for locale in ["zh_cn", "en_us", "ja_jp"] {
        if let Some(document) = value.get(locale).filter(|value| is_post_document(value)) {
            return Some(document);
        }
    }

    if has_content_v2(value) {
        return Some(value);
    }

    value
        .as_object()?
        .values()
        .find(|value| is_post_document(value))
}

fn is_post_document(value: &Value) -> bool {
    has_legacy_post_content(value) || has_content_v2(value)
}

fn has_legacy_post_content(value: &Value) -> bool {
    value.get("title").and_then(Value::as_str).is_some()
        || value.get("content").and_then(Value::as_array).is_some()
}

fn has_content_v2(value: &Value) -> bool {
    value.get("content_v2").and_then(Value::as_array).is_some()
}

fn post_document_text(document: &Value) -> String {
    let mut lines = Vec::new();

    if let Some(title) = document.get("title").and_then(Value::as_str) {
        if !title.is_empty() {
            lines.push(title.to_owned());
        }
    }

    if let Some(content) = post_text_content(document) {
        lines.extend(
            content
                .iter()
                .filter_map(post_line_text)
                .filter(|line| !line.is_empty()),
        );
    }

    lines.join("\n")
}

fn post_text_content(document: &Value) -> Option<&[Value]> {
    let content = document.get("content").and_then(Value::as_array);
    if let Some(lines) = content {
        if !lines.is_empty() {
            return Some(lines.as_slice());
        }
    }

    let content_v2 = document.get("content_v2").and_then(Value::as_array);
    if let Some(lines) = content_v2 {
        if !lines.is_empty() {
            return Some(lines.as_slice());
        }
    }

    content.map(Vec::as_slice)
}

fn post_line_text(line: &Value) -> Option<String> {
    let elements = line.as_array()?;
    let mut text = String::new();

    for element in elements {
        text.push_str(&post_element_text(element));
    }

    Some(text)
}

fn post_element_text(element: &Value) -> String {
    match element.get("tag").and_then(Value::as_str) {
        Some("at") => post_at_text(element),
        Some("text" | "a") => element_text(element),
        _ => element_text(element),
    }
}

fn element_text(element: &Value) -> String {
    element
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn post_at_text(element: &Value) -> String {
    let name = element
        .get("user_name")
        .or_else(|| element.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if name.is_empty() || name.starts_with('@') {
        name.to_owned()
    } else {
        format!("@{name}")
    }
}

fn post_at_mention(element: &Value) -> Option<MessageMention> {
    if element.get("tag").and_then(Value::as_str) != Some("at") {
        return None;
    }

    let raw_user_id = element
        .get("user_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = element
        .get("user_name")
        .or_else(|| element.get("text"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);

    if raw_user_id.is_empty() {
        return None;
    }

    if raw_user_id == "all" || raw_user_id == "all_members" {
        return Some(MessageMention {
            key: "@_all".to_owned(),
            open_id: String::new(),
            user_id: None,
            union_id: None,
            name,
            mentioned_type: MessageSenderType::Unknown,
        });
    }

    let (open_id, user_id) = if raw_user_id.starts_with("ou_") {
        (raw_user_id.to_owned(), None)
    } else {
        (String::new(), Some(raw_user_id.to_owned()))
    };

    Some(MessageMention {
        key: String::new(),
        open_id,
        user_id,
        union_id: None,
        name,
        mentioned_type: MessageSenderType::Unknown,
    })
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

fn resolve_mention_keys(text: String, mentions: &[MessageMention]) -> String {
    let replacements = mentions
        .iter()
        .filter(|mention| !mention.key.is_empty())
        .map(|mention| (mention.key.as_str(), mention_display_text(mention)))
        .filter(|(key, replacement)| key != replacement)
        .collect::<Vec<_>>();

    if replacements.is_empty() {
        return text;
    }

    let mut resolved = String::with_capacity(text.len());
    let mut index = 0;

    while index < text.len() {
        let remaining = &text[index..];
        if let Some((key, replacement)) = replacements
            .iter()
            .filter(|(key, _)| remaining_starts_with_mention_key(remaining, key))
            .max_by_key(|(key, _)| key.len())
        {
            resolved.push_str(replacement);
            index += key.len();
        } else if let Some(next) = remaining.chars().next() {
            resolved.push(next);
            index += next.len_utf8();
        } else {
            break;
        }
    }

    resolved
}

fn remaining_starts_with_mention_key(remaining: &str, key: &str) -> bool {
    if !remaining.starts_with(key) {
        return false;
    }

    match remaining[key.len()..].chars().next() {
        Some(next) => !is_mention_key_char(next),
        None => true,
    }
}

fn is_mention_key_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}

fn mention_display_text(mention: &MessageMention) -> String {
    if let Some(name) = mention.name.as_deref().filter(|name| !name.is_empty()) {
        if name.starts_with('@') {
            name.to_owned()
        } else {
            format!("@{name}")
        }
    } else {
        mention.key.clone()
    }
}

fn push_message_mention(mentions: &mut Vec<MessageMention>, mention: MessageMention) {
    if mention.key.is_empty()
        && mention.open_id.is_empty()
        && mention.user_id.is_none()
        && mention.union_id.is_none()
    {
        return;
    }

    if let Some(existing) = mentions
        .iter_mut()
        .find(|existing| same_message_mention(existing, &mention))
    {
        merge_message_mention(existing, mention);
    } else {
        mentions.push(mention);
    }
}

fn same_message_mention(left: &MessageMention, right: &MessageMention) -> bool {
    (!left.key.is_empty() && left.key == right.key)
        || (!left.open_id.is_empty() && left.open_id == right.open_id)
        || same_non_empty_optional_id(left.user_id.as_deref(), right.user_id.as_deref())
        || same_non_empty_optional_id(left.union_id.as_deref(), right.union_id.as_deref())
}

fn same_non_empty_optional_id(left: Option<&str>, right: Option<&str>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if !left.is_empty() && left == right)
}

fn merge_message_mention(existing: &mut MessageMention, next: MessageMention) {
    if existing.key.is_empty() {
        existing.key = next.key;
    }
    if existing.open_id.is_empty() {
        existing.open_id = next.open_id;
    }
    if existing.user_id.is_none() {
        existing.user_id = next.user_id;
    }
    if existing.union_id.is_none() {
        existing.union_id = next.union_id;
    }
    if existing.name.is_none() {
        existing.name = next.name;
    }
    if existing.mentioned_type == MessageSenderType::Unknown {
        existing.mentioned_type = next.mentioned_type;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn mention(key: &str, open_id: &str, name: &str) -> MessageMention {
        MessageMention {
            key: key.to_owned(),
            open_id: open_id.to_owned(),
            user_id: None,
            union_id: None,
            name: Some(name.to_owned()),
            mentioned_type: MessageSenderType::Unknown,
        }
    }

    #[test]
    fn resolves_overlapping_mention_keys_without_corrupting_longer_keys() {
        let content = json!({
            "text": "@_user_10 then @_user_1"
        });
        let mentions = vec![
            mention("@_user_1", "ou_short", "Short"),
            mention("@_user_10", "ou_long", "Long"),
        ];

        assert_eq!(
            normalize_message_text("text", Some(&content), &mentions),
            "@Long then @Short"
        );
    }

    #[test]
    fn resolves_duplicate_mention_aliases_before_deduplicating_public_mentions() {
        let content = json!({
            "text": "@_user_1 then @_user_2"
        });
        let mentions = vec![
            mention("@_user_1", "ou_bot", "Bot"),
            mention("@_user_2", "ou_bot", "Bot"),
        ];

        assert_eq!(
            normalize_message_text("text", Some(&content), &mentions),
            "@Bot then @Bot"
        );

        let public_mentions = normalize_message_mentions("text", Some(&content), mentions);
        assert_eq!(public_mentions.len(), 1);
        assert_eq!(public_mentions[0].key, "@_user_1");
        assert_eq!(public_mentions[0].open_id, "ou_bot");
    }

    #[test]
    fn does_not_resolve_known_mention_key_inside_unknown_longer_key() {
        let content = json!({
            "text": "@_user_10 then @_user_1"
        });
        let mentions = vec![mention("@_user_1", "ou_short", "Short")];

        assert_eq!(
            normalize_message_text("text", Some(&content), &mentions),
            "@_user_10 then @Short"
        );
    }

    #[test]
    fn normalizes_lark_post_message_to_plain_text() {
        let content = json!({
            "title": "Post title",
            "content": [
                [
                    {
                        "tag": "text",
                        "text": "hello "
                    },
                    {
                        "tag": "at",
                        "user_id": "ou_bot",
                        "user_name": "Bot"
                    },
                    {
                        "tag": "text",
                        "text": " see "
                    },
                    {
                        "tag": "a",
                        "text": "docs",
                        "href": "https://example.test"
                    }
                ],
                [
                    {
                        "tag": "text",
                        "text": "second line"
                    },
                    {
                        "tag": "img",
                        "image_key": "img_v2_1"
                    }
                ]
            ]
        });

        assert_eq!(
            normalize_message_text("post", Some(&content), &[]),
            "Post title\nhello @Bot see docs\nsecond line"
        );

        let mentions = normalize_message_mentions("post", Some(&content), Vec::new());
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].open_id, "ou_bot");
        assert_eq!(mentions[0].name.as_deref(), Some("Bot"));
    }

    #[test]
    fn normalizes_content_v2_only_post_text_and_mentions() {
        let content = json!({
            "title": "Post title",
            "content_v2": [
                [
                    {
                        "tag": "text",
                        "text": "hello "
                    },
                    {
                        "tag": "at",
                        "user_id": "ou_bot",
                        "user_name": "Bot"
                    },
                    {
                        "tag": "a",
                        "text": " docs",
                        "href": "https://example.test"
                    }
                ]
            ]
        });

        assert_eq!(
            normalize_message_text("post", Some(&content), &[]),
            "Post title\nhello @Bot docs"
        );

        let mentions = normalize_message_mentions("post", Some(&content), Vec::new());
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].open_id, "ou_bot");
        assert_eq!(mentions[0].name.as_deref(), Some("Bot"));
    }

    #[test]
    fn deduplicates_lark_post_mentions_against_event_metadata() {
        let content = json!({
            "title": "",
            "content": [[
                {
                    "tag": "at",
                    "user_id": "ou_bot",
                    "user_name": "Bot"
                }
            ]]
        });
        let event_mentions = vec![MessageMention {
            key: "@_user_1".to_owned(),
            open_id: "ou_bot".to_owned(),
            user_id: None,
            union_id: None,
            name: None,
            mentioned_type: MessageSenderType::Unknown,
        }];

        let mentions = normalize_message_mentions("post", Some(&content), event_mentions);
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].key, "@_user_1");
        assert_eq!(mentions[0].open_id, "ou_bot");
        assert_eq!(mentions[0].name.as_deref(), Some("Bot"));
    }

    #[test]
    fn does_not_deduplicate_distinct_mentions_with_empty_optional_ids() {
        let content = json!({
            "text": "@_a @_b"
        });
        let mentions = vec![mention("@_a", "ou_a", "A"), mention("@_b", "ou_b", "B")];

        let mentions = normalize_message_mentions("text", Some(&content), mentions);
        assert_eq!(mentions.len(), 2);
        assert_eq!(mentions[0].open_id, "ou_a");
        assert_eq!(mentions[0].user_id, None);
        assert_eq!(mentions[0].union_id, None);
        assert_eq!(mentions[1].open_id, "ou_b");
        assert_eq!(mentions[1].user_id, None);
        assert_eq!(mentions[1].union_id, None);
    }

    #[test]
    fn skips_rich_text_at_mentions_without_identifier() {
        let content = json!({
            "title": "",
            "content": [[
                {
                    "tag": "at",
                    "user_name": "Visible"
                }
            ]]
        });

        assert_eq!(
            normalize_message_text("post", Some(&content), &[]),
            "@Visible"
        );
        assert!(normalize_message_mentions("post", Some(&content), Vec::new()).is_empty());
    }

    #[test]
    fn normalizes_multilingual_lark_post_message_with_preferred_locale() {
        let content = json!({
            "en_us": {
                "title": "English",
                "content": [[
                    {
                        "tag": "text",
                        "text": "hello"
                    }
                ]]
            },
            "zh_cn": {
                "title": "中文",
                "content": [[
                    {
                        "tag": "text",
                        "text": "你好"
                    }
                ]]
            }
        });

        assert_eq!(
            normalize_message_text("post", Some(&content), &[]),
            "中文\n你好"
        );
    }

    #[test]
    fn preferred_locale_wins_over_top_level_content_v2() {
        let content = json!({
            "content_v2": [[
                {
                    "tag": "text",
                    "text": "top level fallback"
                }
            ]],
            "zh_cn": {
                "title": "中文",
                "content": [[
                    {
                        "tag": "text",
                        "text": "你好"
                    }
                ]]
            }
        });

        assert_eq!(
            normalize_message_text("post", Some(&content), &[]),
            "中文\n你好"
        );
    }

    #[test]
    fn normalizes_lark_post_message_with_locale_fallbacks() {
        let ja_jp = json!({
            "ja_jp": {
                "title": "日本語",
                "content": [[
                    {
                        "tag": "text",
                        "text": "こんにちは"
                    }
                ]]
            }
        });

        assert_eq!(parse_post_content(Some(&ja_jp)), "日本語\nこんにちは");

        let en_us_over_ja_jp = json!({
            "ja_jp": {
                "title": "日本語",
                "content": [[
                    {
                        "tag": "text",
                        "text": "こんにちは"
                    }
                ]]
            },
            "en_us": {
                "title": "English",
                "content": [[
                    {
                        "tag": "text",
                        "text": "hello"
                    }
                ]]
            }
        });

        assert_eq!(
            parse_post_content(Some(&en_us_over_ja_jp)),
            "English\nhello"
        );

        let custom_locale = json!({
            "fr_fr": {
                "title": "Français",
                "content": [[
                    {
                        "tag": "text",
                        "text": "bonjour"
                    }
                ]]
            }
        });

        assert_eq!(
            parse_post_content(Some(&custom_locale)),
            "Français\nbonjour"
        );
    }

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
