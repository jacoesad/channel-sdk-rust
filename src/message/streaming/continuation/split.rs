use std::collections::VecDeque;

use crate::{Error, Result};

use super::super::{MarkdownStreamCardProfile, validate_stream_content_states};

pub(in crate::message::streaming) const DEFAULT_CONTINUATION_MAX_PAGE_CHARS: usize = 30_000;

#[derive(Debug)]
pub(super) struct PlannedPage {
    pub(super) content: String,
}

pub(in crate::message::streaming) fn validate_continuation_limit(
    max_page_chars: usize,
) -> Result<()> {
    if max_page_chars == 0 {
        return Err(Error::Validation(
            "markdown continuation page limit must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn plan_content(
    source: &str,
    profile: &MarkdownStreamCardProfile,
    max_page_chars: usize,
    minimum_first_page_source_bytes: usize,
) -> Result<VecDeque<PlannedPage>> {
    debug_assert!(!source.is_empty());

    let mut pages = VecDeque::new();
    let mut remaining = source;
    let mut minimum_source_bytes = minimum_first_page_source_bytes;

    while !remaining.is_empty() {
        if page_fits(remaining, profile, max_page_chars) {
            pages.push_back(page(remaining));
            break;
        }

        let boundary =
            choose_split_boundary(remaining, profile, max_page_chars, minimum_source_bytes)?;
        pages.push_back(page(&remaining[..boundary]));
        remaining = &remaining[boundary..];
        minimum_source_bytes = 0;
    }

    Ok(pages)
}

fn page(source: &str) -> PlannedPage {
    PlannedPage {
        content: source.to_owned(),
    }
}

fn choose_split_boundary(
    source: &str,
    profile: &MarkdownStreamCardProfile,
    max_page_chars: usize,
    minimum_source_bytes: usize,
) -> Result<usize> {
    let boundaries = candidate_boundaries(source, max_page_chars);
    let max_fitting = largest_fitting_boundary(source, &boundaries, profile, max_page_chars)
        .ok_or_else(page_too_small)?;

    if max_fitting < minimum_source_bytes {
        return Err(Error::Validation(
            "markdown continuation cannot move previously accepted content to a new page"
                .to_owned(),
        ));
    }

    let minimum_preferred = (max_fitting.saturating_mul(3) / 5).max(minimum_source_bytes);
    let mut eligible = boundaries
        .iter()
        .copied()
        .filter(|boundary| *boundary <= max_fitting && *boundary >= minimum_source_bytes);

    for preference in [
        BoundaryPreference::Paragraph,
        BoundaryPreference::Line,
        BoundaryPreference::Whitespace,
    ] {
        if let Some(boundary) = eligible.clone().rev().find(|boundary| {
            *boundary >= minimum_preferred && preference.matches(source, *boundary)
        }) {
            return Ok(boundary);
        }
    }

    eligible.next_back().ok_or_else(page_too_small)
}

#[derive(Clone, Copy)]
enum BoundaryPreference {
    Paragraph,
    Line,
    Whitespace,
}

impl BoundaryPreference {
    fn matches(self, source: &str, boundary: usize) -> bool {
        let prefix = &source[..boundary];
        match self {
            Self::Paragraph => {
                prefix.ends_with("\n\n") || prefix.ends_with("\r\r") || prefix.ends_with("\r\n\r\n")
            }
            Self::Line => prefix.ends_with(['\n', '\r']),
            Self::Whitespace => prefix.chars().next_back().is_some_and(char::is_whitespace),
        }
    }
}

fn candidate_boundaries(source: &str, max_page_chars: usize) -> Vec<usize> {
    source
        .char_indices()
        .map(|(index, _)| index)
        .skip(1)
        .chain(std::iter::once(source.len()))
        .take(max_page_chars)
        .collect()
}

fn largest_fitting_boundary(
    source: &str,
    boundaries: &[usize],
    profile: &MarkdownStreamCardProfile,
    max_page_chars: usize,
) -> Option<usize> {
    let mut low = 0;
    let mut high = boundaries.len();
    let mut best = None;

    while low < high {
        let mid = (low + high) / 2;
        let boundary = boundaries[mid];
        if page_fits(&source[..boundary], profile, max_page_chars) {
            best = Some(boundary);
            low = mid + 1;
        } else {
            high = mid;
        }
    }

    best
}

fn page_fits(source: &str, profile: &MarkdownStreamCardProfile, max_page_chars: usize) -> bool {
    source.chars().count() <= max_page_chars
        && validate_stream_content_states(source, profile).is_ok()
}

fn page_too_small() -> Error {
    Error::Validation("markdown continuation page limit is too small for content".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_preserves_unicode_and_complete_source() {
        let profile = MarkdownStreamCardProfile::default();
        let source = "界".repeat(80);

        let pages = plan_content(&source, &profile, 16, 0).expect("page plan");

        assert!(pages.len() > 1);
        assert!(pages.iter().all(|page| page.content.chars().count() <= 16));
        assert_eq!(reconstructed_source(&pages), source);
    }

    #[test]
    fn prefers_natural_boundaries() {
        let profile = MarkdownStreamCardProfile::default();
        let pages = plan_content("first paragraph\n\nsecond paragraph", &profile, 20, 0)
            .expect("page plan");

        assert_eq!(pages[0].content, "first paragraph\n\n");
        assert_eq!(pages[1].content, "second paragraph");
    }

    #[test]
    fn markdown_is_split_without_rewriting_source() {
        let profile = MarkdownStreamCardProfile::default();
        let source = "```text\nabcdefghijklmno\n```";

        let pages = plan_content(source, &profile, 12, 0).expect("format-agnostic page plan");

        assert!(pages.len() > 1);
        assert_eq!(reconstructed_source(&pages), source);
    }

    #[test]
    fn later_fence_text_can_cross_the_existing_page_boundary() {
        let profile = MarkdownStreamCardProfile::default();
        let initial = "1234567\n```\na";
        let complete = format!("{initial}b\n```");

        let pages = plan_content(&complete, &profile, 16, initial.len())
            .expect("format-agnostic continuation accepts later fence text");

        assert!(pages.len() > 1);
        assert_eq!(reconstructed_source(&pages), complete);
        assert!(pages[0].content.len() >= initial.len());
    }

    #[test]
    fn later_line_feed_can_follow_a_page_ending_carriage_return() {
        let profile = MarkdownStreamCardProfile::default();
        let initial = "ab\r";
        let complete = "ab\r\n";

        let pages = plan_content(complete, &profile, 3, initial.len())
            .expect("format-agnostic continuation accepts a split CRLF pair");

        assert_eq!(reconstructed_source(&pages), complete);
        assert_eq!(pages[0].content, initial);
        assert_eq!(pages[1].content, "\n");
    }

    #[test]
    fn serialized_card_limit_splits_plain_text_without_losing_source() {
        let profile = MarkdownStreamCardProfile::default();
        let source = "\\".repeat(40_000);

        let pages = plan_content(&source, &profile, 100_000, 0)
            .expect("serialized size pressure produces multiple pages");

        assert!(pages.len() > 1);
        assert_eq!(reconstructed_source(&pages), source);
        assert!(
            pages
                .iter()
                .all(|page| { validate_stream_content_states(&page.content, &profile).is_ok() })
        );
    }

    #[test]
    fn accepted_content_is_not_moved_to_a_follow_up_page() {
        let profile = MarkdownStreamCardProfile::default();

        let error = plan_content("abcdefghijk", &profile, 8, 9)
            .expect_err("the accepted prefix no longer fits");

        assert!(matches!(
            error,
            Error::Validation(message) if message.contains("previously accepted")
        ));
    }

    #[test]
    fn zero_limit_is_rejected() {
        assert!(validate_continuation_limit(0).is_err());
        assert!(validate_continuation_limit(1).is_ok());
    }

    fn reconstructed_source(pages: &VecDeque<PlannedPage>) -> String {
        pages.iter().map(|page| page.content.as_str()).collect()
    }
}
