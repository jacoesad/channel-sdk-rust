//! High-level Markdown streaming through CardKit entities.

mod builder;
mod stream;
#[cfg(test)]
mod test_support;

pub use builder::MarkdownStreamBuilder;
pub use stream::MarkdownStream;

use crate::card::{CardElementContent, CardStreamingConfig};
use crate::{Card, CardElement, Result};

const STREAM_ELEMENT_ID: &str = "stream_md";
const DEFAULT_INITIAL_TEXT: &str = "Thinking...";
const DEFAULT_STREAMING_SUMMARY: &str = "[Generating...]";
const DEFAULT_EMPTY_TEXT: &str = "(no content)";
const MAX_DERIVED_SUMMARY_CHARS: usize = 50;

#[derive(Debug, Clone)]
struct MarkdownStreamCardProfile {
    initial_text: String,
    empty_text: String,
    streaming_summary: String,
    final_summary: Option<String>,
    streaming_config: CardStreamingConfig,
}

impl Default for MarkdownStreamCardProfile {
    fn default() -> Self {
        Self {
            initial_text: DEFAULT_INITIAL_TEXT.to_owned(),
            empty_text: DEFAULT_EMPTY_TEXT.to_owned(),
            streaming_summary: DEFAULT_STREAMING_SUMMARY.to_owned(),
            final_summary: None,
            streaming_config: CardStreamingConfig::new(),
        }
    }
}

fn validate_stream_content_states(
    content: &str,
    profile: &MarkdownStreamCardProfile,
) -> Result<CardElementContent> {
    let content = CardElementContent::new(content)?;
    build_streaming_card(
        content.as_str(),
        true,
        &profile.streaming_summary,
        &profile.streaming_config,
    )?;
    build_streaming_card(
        content.as_str(),
        false,
        &profile
            .final_summary
            .clone()
            .unwrap_or_else(|| derive_summary(content.as_str())),
        &profile.streaming_config,
    )?;
    Ok(content)
}

fn build_streaming_card(
    content: &str,
    streaming_mode: bool,
    summary: &str,
    config: &CardStreamingConfig,
) -> Result<Card> {
    let content = CardElementContent::new(content)?;
    let element = CardElement::markdown(content.as_str()).element_id(STREAM_ELEMENT_ID)?;
    Card::builder()
        .streaming(config.clone())
        .streaming_mode(streaming_mode)
        .summary(summary)
        .element(element)
        .build()
}

fn derive_summary(content: &str) -> String {
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_DERIVED_SUMMARY_CHARS {
        return compact;
    }

    compact
        .chars()
        .take(MAX_DERIVED_SUMMARY_CHARS - 3)
        .chain("...".chars())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_summary_is_compact_and_unicode_safe() {
        assert_eq!(derive_summary("  hello\n\nworld  "), "hello world");

        let summary = derive_summary(&"界".repeat(60));
        assert_eq!(summary.chars().count(), MAX_DERIVED_SUMMARY_CHARS);
        assert!(summary.ends_with("..."));
    }
}
