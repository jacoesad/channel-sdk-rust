use serde_json::Value;

use crate::message::{MessageMention, MessageSenderType};

use super::post::{post_text_content, select_post_document};

pub(in crate::event) fn normalize_message_mentions(
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
    fn deduplicates_duplicate_event_mentions_by_identity() {
        let content = json!({
            "text": "@_user_1 then @_user_2"
        });
        let mentions = vec![
            mention("@_user_1", "ou_bot", "Bot"),
            mention("@_user_2", "ou_bot", "Bot"),
        ];

        let public_mentions = normalize_message_mentions("text", Some(&content), mentions);
        assert_eq!(public_mentions.len(), 1);
        assert_eq!(public_mentions[0].key, "@_user_1");
        assert_eq!(public_mentions[0].open_id, "ou_bot");
    }

    #[test]
    fn normalizes_lark_post_message_mentions() {
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
                    }
                ]
            ]
        });

        let mentions = normalize_message_mentions("post", Some(&content), Vec::new());
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].open_id, "ou_bot");
        assert_eq!(mentions[0].name.as_deref(), Some("Bot"));
    }

    #[test]
    fn normalizes_content_v2_only_post_mentions() {
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
                    }
                ]
            ]
        });

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

        assert!(normalize_message_mentions("post", Some(&content), Vec::new()).is_empty());
    }
}
