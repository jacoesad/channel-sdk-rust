use serde_json::Value;

use crate::message::MessageMention;

use super::post::{post_document_text, select_post_document};

pub(in crate::event) fn normalize_message_text(
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

fn parse_text_content(content: Option<&Value>) -> String {
    content
        .and_then(|value| value.get("text").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default()
}

fn parse_post_content(content: Option<&Value>) -> String {
    content
        .and_then(select_post_document)
        .map(post_document_text)
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::message::{MessageMention, MessageSenderType};

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
    fn resolves_duplicate_mention_aliases_before_public_deduplication() {
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
    }

    #[test]
    fn normalizes_content_v2_only_post_text() {
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
    }

    #[test]
    fn keeps_post_at_text_when_identifier_is_missing() {
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
}
