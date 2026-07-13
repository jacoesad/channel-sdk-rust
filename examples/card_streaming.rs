use std::env;
use std::io;

use lark_channel::lark_openapi::{CardUpdateOptions, OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{
    Card, CardElement, CardElementContent, CardSettings, CardStreamingConfig, ChannelConfig, Error,
    MessageSender, Recipient,
};

const STREAM_ELEMENT_ID: &str = "stream_text";
const STREAMING_SUMMARY: &str = "[Generating...]";
const FINAL_SUMMARY: &str = "Streaming update completed";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamElementKind {
    Markdown,
    PlainText,
}

impl StreamElementKind {
    fn from_env() -> lark_channel::Result<Self> {
        match env::var("LARK_STREAM_ELEMENT") {
            Ok(value) => Self::parse(Some(&value)),
            Err(env::VarError::NotPresent) => Self::parse(None),
            Err(env::VarError::NotUnicode(_)) => Err(Error::Validation(
                "LARK_STREAM_ELEMENT must be valid Unicode".to_owned(),
            )),
        }
    }

    fn parse(value: Option<&str>) -> lark_channel::Result<Self> {
        match value {
            None | Some("markdown") => Ok(Self::Markdown),
            Some("plain_text") => Ok(Self::PlainText),
            Some(_) => Err(Error::Validation(
                "LARK_STREAM_ELEMENT must be `markdown` or `plain_text`".to_owned(),
            )),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ChannelConfig::new(
        required_env("LARK_APP_ID")?,
        required_env("LARK_APP_SECRET")?,
    );
    let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
    let sender = MessageSender::new(openapi);
    let recipient = recipient_from_env()?;
    let element_kind = StreamElementKind::from_env()?;
    let content = validate_stream_content(
        env::var("LARK_STREAM_TEXT")
            .unwrap_or_else(|_| "CardKit streaming API smoke test completed.".to_owned()),
    )?;
    preflight_streaming_card_states(element_kind, &content)?;

    let card = streaming_card(
        element_kind,
        initial_stream_prefix(&content),
        true,
        STREAMING_SUMMARY,
    )?;
    let card_id = sender.client().create_card_entity(&card).await?;
    let message_id = sender
        .card_reference_message(recipient, card_id.clone())
        .send()
        .await?;

    sender
        .client()
        .update_card_element_content(
            &card_id,
            STREAM_ELEMENT_ID,
            content.as_str(),
            CardUpdateOptions::new(1).uuid(format!("content-{}-1", card_id.as_str())),
        )
        .await?;
    sender
        .client()
        .update_card_settings(
            &card_id,
            &CardSettings::new()
                .streaming_mode(false)
                .summary(FINAL_SUMMARY),
            CardUpdateOptions::new(2).uuid(format!("settings-{}-2", card_id.as_str())),
        )
        .await?;

    println!(
        "streaming card completed: card_id={}, message_id={}",
        card_id.as_str(),
        message_id.0
    );
    Ok(())
}

fn streaming_card(
    element_kind: StreamElementKind,
    content: impl Into<String>,
    streaming_mode: bool,
    summary: &str,
) -> lark_channel::Result<Card> {
    let content = content.into();
    let element = match element_kind {
        StreamElementKind::Markdown => {
            CardElement::markdown(content).element_id(STREAM_ELEMENT_ID)?
        }
        StreamElementKind::PlainText => {
            CardElement::text(content).plain_text_element_id(STREAM_ELEMENT_ID)?
        }
    };
    Card::builder()
        .streaming(CardStreamingConfig::new())
        .streaming_mode(streaming_mode)
        .summary(summary)
        .element(element)
        .build()
}

fn preflight_streaming_card_states(
    element_kind: StreamElementKind,
    content: &CardElementContent,
) -> lark_channel::Result<()> {
    streaming_card(element_kind, content.as_str(), true, STREAMING_SUMMARY)?;
    streaming_card(element_kind, content.as_str(), false, FINAL_SUMMARY)?;
    Ok(())
}

fn recipient_from_env() -> Result<Recipient, io::Error> {
    if let Ok(chat_id) = env::var("LARK_CHAT_ID") {
        return Ok(Recipient::Chat(chat_id));
    }
    if let Ok(open_id) = env::var("LARK_OPEN_ID") {
        return Ok(Recipient::User(open_id));
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "missing required environment variable: LARK_CHAT_ID or LARK_OPEN_ID",
    ))
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable: {name}"),
        )
    })
}

fn validate_stream_content(content: String) -> lark_channel::Result<CardElementContent> {
    let content = CardElementContent::new(content)?;
    if content.as_str().chars().count() < 2 {
        return Err(Error::Validation(
            "LARK_STREAM_TEXT must contain at least two characters".to_owned(),
        ));
    }
    Ok(content)
}

fn initial_stream_prefix(content: &CardElementContent) -> String {
    content.as_str().chars().take(1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lark_channel::MAX_CARD_JSON_BYTES;

    #[test]
    fn parses_supported_stream_element_kinds() {
        assert_eq!(
            StreamElementKind::parse(None).expect("default kind"),
            StreamElementKind::Markdown
        );
        assert_eq!(
            StreamElementKind::parse(Some("markdown")).expect("Markdown kind"),
            StreamElementKind::Markdown
        );
        assert_eq!(
            StreamElementKind::parse(Some("plain_text")).expect("plain-text kind"),
            StreamElementKind::PlainText
        );
    }

    #[test]
    fn rejects_unknown_stream_element_kind() {
        let error =
            StreamElementKind::parse(Some("text")).expect_err("unknown element kind must fail");

        assert!(matches!(error, Error::Validation(_)));
    }

    #[test]
    fn builds_markdown_stream_target_on_the_top_level_element() {
        let card = streaming_card(
            StreamElementKind::Markdown,
            "Thinking...",
            true,
            STREAMING_SUMMARY,
        )
        .expect("Markdown streaming card");
        let element = &card.as_value()["body"]["elements"][0];

        assert_eq!(element["tag"], "markdown");
        assert_eq!(element["element_id"], STREAM_ELEMENT_ID);
        assert_eq!(element["content"], "Thinking...");
        assert!(element.get("text").is_none());
    }

    #[test]
    fn builds_plain_text_stream_target_on_the_nested_text_element() {
        let card = streaming_card(
            StreamElementKind::PlainText,
            "Thinking...",
            true,
            STREAMING_SUMMARY,
        )
        .expect("plain-text streaming card");
        let element = &card.as_value()["body"]["elements"][0];

        assert_eq!(element["tag"], "div");
        assert!(element.get("element_id").is_none());
        assert_eq!(element["text"]["tag"], "plain_text");
        assert_eq!(element["text"]["element_id"], STREAM_ELEMENT_ID);
        assert_eq!(element["text"]["content"], "Thinking...");
    }

    #[test]
    fn initial_text_is_a_unicode_safe_proper_prefix() {
        let content = validate_stream_content("你好".to_owned()).expect("content");
        let initial = initial_stream_prefix(&content);

        assert_eq!(initial, "你");
        assert!(content.as_str().starts_with(&initial));
        assert_ne!(content.as_str(), initial);
    }

    #[test]
    fn accepts_final_card_within_the_platform_size_limit() {
        let content = validate_stream_content("complete output".to_owned()).expect("content");

        for element_kind in [StreamElementKind::Markdown, StreamElementKind::PlainText] {
            preflight_streaming_card_states(element_kind, &content)
                .expect("card states within limit");
        }
    }

    #[test]
    fn rejects_content_without_an_appendable_suffix() {
        for content in ["", "x", "界"] {
            let error = validate_stream_content(content.to_owned())
                .expect_err("content must have at least two characters");
            assert!(matches!(error, Error::Validation(_)));
        }
    }

    #[test]
    fn rejects_oversized_unicode_content_before_the_example_can_send() {
        let error =
            validate_stream_content("界".repeat(100_001)).expect_err("oversized content must fail");

        assert!(matches!(error, Error::Validation(_)));
    }

    #[test]
    fn rejects_final_card_over_30_kib_before_the_example_can_send() {
        let content = validate_stream_content("x".repeat(31_000)).expect("content field limit");
        let error = preflight_streaming_card_states(StreamElementKind::Markdown, &content)
            .expect_err("oversized card must fail");

        assert!(matches!(error, Error::Validation(_)));
    }

    #[test]
    fn preflight_checks_the_larger_closed_card_state() {
        let empty_streaming =
            streaming_card(StreamElementKind::Markdown, "", true, STREAMING_SUMMARY)
                .expect("empty card");
        let streaming_overhead = serde_json::to_vec(empty_streaming.as_value())
            .expect("streaming card JSON")
            .len();
        let content = validate_stream_content("x".repeat(MAX_CARD_JSON_BYTES - streaming_overhead))
            .expect("exact-limit streaming content");
        let streaming = streaming_card(
            StreamElementKind::Markdown,
            content.as_str(),
            true,
            STREAMING_SUMMARY,
        )
        .expect("streaming state reaches the exact limit");
        assert_eq!(
            serde_json::to_vec(streaming.as_value())
                .expect("streaming card JSON")
                .len(),
            MAX_CARD_JSON_BYTES
        );

        let error = preflight_streaming_card_states(StreamElementKind::Markdown, &content)
            .expect_err("larger closed state must fail preflight");
        assert!(matches!(error, Error::Validation(_)));
    }
}
