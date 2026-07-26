use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use url::Url;

use crate::{Error, Result};

const DEFAULT_POST_LOCALE: &str = "zh_cn";
const ENGLISH_POST_LOCALE: &str = "en_us";

/// Lark/Feishu rich-text `post` message content.
///
/// The serialized shape is a locale map such as
/// `{ "zh_cn": { "title": "", "content": [[...]] } }`.
#[derive(Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PostContent {
    locales: BTreeMap<String, PostDocument>,
}

impl fmt::Debug for PostContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostContent")
            .field("locales_len", &self.locales.len())
            .finish()
    }
}

impl<'de> Deserialize<'de> for PostContent {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let content = Self {
            locales: BTreeMap::deserialize(deserializer)?,
        };
        content.validate().map_err(D::Error::custom)?;
        Ok(content)
    }
}

impl PostContent {
    /// Starts a rich-text builder using the default `zh_cn` locale.
    pub fn builder() -> PostContentBuilder {
        PostContentBuilder::new()
    }

    /// Wraps Markdown in the native Lark/Feishu `md` post element.
    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self::from_default_document(PostDocument::new().markdown(markdown))
    }

    /// Wraps plain text in a structured Lark/Feishu `text` post element.
    pub fn text(text: impl Into<String>) -> Self {
        Self::from_default_document(PostDocument::new().paragraph([PostElement::text(text)]))
    }

    /// Creates validated content for the default `zh_cn` locale.
    pub fn new(document: PostDocument) -> Result<Self> {
        Self::for_locale(DEFAULT_POST_LOCALE, document)
    }

    /// Creates content for the documented `zh_cn` or `en_us` locale.
    pub fn for_locale(locale: impl Into<String>, document: PostDocument) -> Result<Self> {
        let locale = locale.into();
        validate_locale_and_document(&locale, &document).map(|locale| {
            let mut locales = BTreeMap::new();
            locales.insert(locale, document);
            Self { locales }
        })
    }

    /// Adds or replaces a documented `zh_cn` or `en_us` localized document.
    pub fn insert_locale(
        &mut self,
        locale: impl Into<String>,
        document: PostDocument,
    ) -> Result<Option<PostDocument>> {
        let locale = locale.into();
        let locale = validate_locale_and_document(&locale, &document)?;
        Ok(self.locales.insert(locale, document))
    }

    /// Returns all localized documents in deterministic key order.
    pub fn locales(&self) -> &BTreeMap<String, PostDocument> {
        &self.locales
    }

    /// Returns one localized document when present.
    pub fn document(&self, locale: &str) -> Option<&PostDocument> {
        self.locales.get(locale)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.locales.is_empty() {
            return Err(Error::Validation(
                "post content must contain at least one locale".to_owned(),
            ));
        }
        for (locale, document) in &self.locales {
            validate_locale_and_document(locale, document)?;
        }
        Ok(())
    }

    fn from_default_document(document: PostDocument) -> Self {
        let mut locales = BTreeMap::new();
        locales.insert(DEFAULT_POST_LOCALE.to_owned(), document);
        Self { locales }
    }
}

/// One localized rich-text document inside a `post` message.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PostDocument {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    title: String,
    content: Vec<Vec<PostElement>>,
}

impl fmt::Debug for PostDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostDocument")
            .field("title_chars", &self.title.chars().count())
            .field("paragraphs_len", &self.content.len())
            .field(
                "elements_len",
                &self.content.iter().map(Vec::len).sum::<usize>(),
            )
            .finish()
    }
}

impl PostDocument {
    /// Creates an empty document.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the optional rich-text title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Appends a paragraph when it contains at least one element.
    pub fn paragraph(mut self, elements: impl IntoIterator<Item = PostElement>) -> Self {
        let elements = elements.into_iter().collect::<Vec<_>>();
        if !elements.is_empty() {
            self.content.push(elements);
        }
        self
    }

    /// Appends a native Markdown paragraph.
    ///
    /// Lark/Feishu requires an `md` element to occupy its own paragraph.
    pub fn markdown(self, markdown: impl Into<String>) -> Self {
        self.paragraph([PostElement::markdown(markdown)])
    }

    /// Returns the optional title as a string slice.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Returns the post paragraphs.
    pub fn paragraphs(&self) -> &[Vec<PostElement>] {
        &self.content
    }

    fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

/// Builder for a single-locale rich-text `post` message.
#[derive(Clone)]
pub struct PostContentBuilder {
    locale: String,
    document: PostDocument,
}

impl fmt::Debug for PostContentBuilder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostContentBuilder")
            .field("locale", &self.locale)
            .field("document", &self.document)
            .finish()
    }
}

impl Default for PostContentBuilder {
    fn default() -> Self {
        Self {
            locale: DEFAULT_POST_LOCALE.to_owned(),
            document: PostDocument::new(),
        }
    }
}

impl PostContentBuilder {
    /// Creates a builder using the default `zh_cn` locale.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects the documented `zh_cn` or `en_us` locale key.
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = locale.into();
        self
    }

    /// Sets the optional rich-text title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.document = self.document.with_title(title);
        self
    }

    /// Appends a structured rich-text paragraph.
    pub fn paragraph(mut self, elements: impl IntoIterator<Item = PostElement>) -> Self {
        self.document = self.document.paragraph(elements);
        self
    }

    /// Appends a native Markdown paragraph.
    pub fn markdown(mut self, markdown: impl Into<String>) -> Self {
        self.document = self.document.markdown(markdown);
        self
    }

    /// Appends a structured plain-text paragraph.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.document = self.document.paragraph([PostElement::text(text)]);
        self
    }

    /// Builds validated rich-text content.
    pub fn build(self) -> Result<PostContent> {
        PostContent::for_locale(self.locale, self.document)
    }
}

/// A supported element in a structured rich-text paragraph.
///
/// Use `MessageContent::Custom` for official post elements that are not yet
/// modeled by these Channel helpers.
#[derive(Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PostElement(PostElementKind);

impl<'de> Deserialize<'de> for PostElement {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let element = Self(PostElementKind::deserialize(deserializer)?);
        element.validate().map_err(D::Error::custom)?;
        Ok(element)
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tag", deny_unknown_fields)]
enum PostElementKind {
    #[serde(rename = "text")]
    Text {
        text: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_optional_bool"
        )]
        un_escape: Option<bool>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        style: Vec<PostStyle>,
    },
    #[serde(rename = "a")]
    Link {
        text: String,
        href: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        style: Vec<PostStyle>,
    },
    #[serde(rename = "at")]
    Mention {
        user_id: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        style: Vec<PostStyle>,
    },
    #[serde(rename = "md")]
    Markdown { text: String },
}

impl fmt::Debug for PostElement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            PostElementKind::Text {
                text,
                un_escape,
                style,
            } => formatter
                .debug_struct("Text")
                .field("chars", &text.chars().count())
                .field("un_escape", un_escape)
                .field("style", style)
                .finish(),
            PostElementKind::Link { text, style, .. } => formatter
                .debug_struct("Link")
                .field("text_chars", &text.chars().count())
                .field("href", &crate::debug::Redacted)
                .field("style", style)
                .finish(),
            PostElementKind::Mention { style, .. } => formatter
                .debug_struct("Mention")
                .field("user_id", &crate::debug::Redacted)
                .field("style", style)
                .finish(),
            PostElementKind::Markdown { text } => formatter
                .debug_struct("Markdown")
                .field("chars", &text.chars().count())
                .finish(),
        }
    }
}

impl PostElement {
    /// Creates a plain text element.
    pub fn text(text: impl Into<String>) -> Self {
        Self(PostElementKind::Text {
            text: text.into(),
            un_escape: None,
            style: Vec::new(),
        })
    }

    /// Creates a validated hyperlink element.
    pub fn link(text: impl Into<String>, href: impl Into<String>) -> Result<Self> {
        let href = href.into();
        Url::parse(&href)?;
        Ok(Self(PostElementKind::Link {
            text: text.into(),
            href,
            style: Vec::new(),
        }))
    }

    /// Creates an @ mention for an open_id, user_id, or union_id.
    pub fn mention(user_id: impl Into<String>) -> Result<Self> {
        let user_id = validate_mention_id(user_id.into())?;
        Ok(Self(PostElementKind::Mention {
            user_id,
            style: Vec::new(),
        }))
    }

    /// Creates an @all mention.
    pub fn mention_all() -> Self {
        Self(PostElementKind::Mention {
            user_id: "all".to_owned(),
            style: Vec::new(),
        })
    }

    /// Creates a native Markdown element.
    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self(PostElementKind::Markdown {
            text: markdown.into(),
        })
    }

    /// Sets the official `un_escape` flag on a plain text element.
    pub fn with_unescape(mut self, unescape: bool) -> Result<Self> {
        match &mut self.0 {
            PostElementKind::Text {
                un_escape: value, ..
            } => *value = Some(unescape),
            _ => {
                return Err(Error::Validation(
                    "post un_escape is only supported for text elements".to_owned(),
                ));
            }
        }
        Ok(self)
    }

    /// Applies supported text styles to text, link, or mention elements.
    pub fn with_styles(mut self, styles: impl IntoIterator<Item = PostStyle>) -> Result<Self> {
        let styles = styles.into_iter().collect::<Vec<_>>();
        let target = match &mut self.0 {
            PostElementKind::Text { style, .. }
            | PostElementKind::Link { style, .. }
            | PostElementKind::Mention { style, .. } => style,
            PostElementKind::Markdown { .. } => {
                return Err(Error::Validation(
                    "post styles are only supported for text, link, and mention elements"
                        .to_owned(),
                ));
            }
        };
        if !styles.is_empty() {
            *target = styles;
        }
        Ok(self)
    }

    fn validate(&self) -> Result<&'static str> {
        match &self.0 {
            PostElementKind::Text { .. } => Ok("text"),
            PostElementKind::Link { href, .. } => {
                Url::parse(href)?;
                Ok("a")
            }
            PostElementKind::Mention { user_id, .. } => {
                validate_mention_id(user_id.clone())?;
                Ok("at")
            }
            PostElementKind::Markdown { .. } => Ok("md"),
        }
    }
}

/// Text styles supported by Lark/Feishu structured post elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostStyle {
    #[serde(rename = "bold")]
    Bold,
    #[serde(rename = "underline")]
    Underline,
    #[serde(rename = "lineThrough")]
    LineThrough,
    #[serde(rename = "italic")]
    Italic,
}

fn validate_locale_and_document(locale: &str, document: &PostDocument) -> Result<String> {
    if !matches!(locale, DEFAULT_POST_LOCALE | ENGLISH_POST_LOCALE) {
        return Err(Error::Validation(
            "post locale must be `zh_cn` or `en_us`".to_owned(),
        ));
    }
    if document.is_empty() {
        return Err(Error::Validation(
            "post content must contain at least one paragraph".to_owned(),
        ));
    }
    for paragraph in &document.content {
        if paragraph.is_empty() {
            return Err(Error::Validation(
                "post paragraphs must contain at least one element".to_owned(),
            ));
        }
        let mut contains_markdown = false;
        for element in paragraph {
            contains_markdown |= element.validate()? == "md";
        }
        if contains_markdown && paragraph.len() != 1 {
            return Err(Error::Validation(
                "post markdown elements must occupy their own paragraph".to_owned(),
            ));
        }
    }
    Ok(locale.to_owned())
}

fn validate_mention_id(user_id: String) -> Result<String> {
    if user_id.trim().is_empty() {
        return Err(Error::Validation(
            "post mention user id must not be empty".to_owned(),
        ));
    }
    if user_id.chars().any(char::is_whitespace) {
        return Err(Error::Validation(
            "post mention user id must not contain whitespace".to_owned(),
        ));
    }
    Ok(user_id)
}

fn deserialize_optional_bool<'de, D>(deserializer: D) -> std::result::Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    bool::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn debug_summarizes_every_public_post_wrapper() {
        let element = PostElement::link(
            "link-secret",
            "https://user:url-secret@example.com/path?token=query-secret#fragment-secret",
        )
        .expect("link");
        let mention = PostElement::mention("mention-secret").expect("mention");
        let document = PostDocument::new()
            .with_title("title-secret")
            .paragraph([element.clone(), mention.clone()]);
        let builder = PostContent::builder()
            .title("builder-secret")
            .paragraph([element.clone(), mention.clone()]);
        let content = PostContent::new(document.clone()).expect("post content");

        let debug = format!("{element:?} {mention:?} {document:?} {builder:?} {content:?}");

        assert!(debug.contains("locales_len: 1"));
        assert!(debug.contains("paragraphs_len: 1"));
        assert!(debug.contains("<redacted>"));
        for secret in [
            "link-secret",
            "url-secret",
            "query-secret",
            "fragment-secret",
            "mention-secret",
            "title-secret",
            "builder-secret",
        ] {
            assert!(!debug.contains(secret));
        }
    }

    #[test]
    fn markdown_uses_native_md_post_shape() {
        let post = PostContent::markdown("# Hello\n\n[docs](https://open.feishu.cn)");

        assert_eq!(
            serde_json::to_value(post).expect("post JSON"),
            json!({
                "zh_cn": {
                    "content": [[{
                        "tag": "md",
                        "text": "# Hello\n\n[docs](https://open.feishu.cn)"
                    }]]
                }
            })
        );
    }

    #[test]
    fn builder_creates_structured_mentions_links_and_styles() {
        let post = PostContent::builder()
            .locale("en_us")
            .title("Status")
            .paragraph([
                PostElement::mention("ou_alice").expect("mention"),
                PostElement::text(" read "),
                PostElement::link("the docs", "https://open.feishu.cn")
                    .expect("link")
                    .with_styles([PostStyle::Bold, PostStyle::Underline])
                    .expect("styles"),
                PostElement::mention_all(),
            ])
            .build()
            .expect("post content");

        assert_eq!(
            serde_json::to_value(post).expect("post JSON"),
            json!({
                "en_us": {
                    "title": "Status",
                    "content": [[
                        { "tag": "at", "user_id": "ou_alice" },
                        { "tag": "text", "text": " read " },
                        {
                            "tag": "a",
                            "text": "the docs",
                            "href": "https://open.feishu.cn",
                            "style": ["bold", "underline"]
                        },
                        { "tag": "at", "user_id": "all" }
                    ]]
                }
            })
        );
    }

    #[test]
    fn rejects_invalid_link_mention_and_empty_document() {
        assert!(matches!(
            PostElement::link("docs", "not a URL"),
            Err(Error::Url(_))
        ));
        assert!(matches!(
            PostElement::mention("  "),
            Err(Error::Validation(message)) if message == "post mention user id must not be empty"
        ));
        assert!(matches!(
            PostContent::builder().build(),
            Err(Error::Validation(message))
                if message == "post content must contain at least one paragraph"
        ));
        assert!(matches!(
            PostContent::new(PostDocument::new()),
            Err(Error::Validation(message))
                if message == "post content must contain at least one paragraph"
        ));
        for locale in ["", "ja_jp", "ZH_CN", "fr-fr", " zh_cn "] {
            assert!(matches!(
                PostContent::builder().locale(locale).text("hello").build(),
                Err(Error::Validation(message))
                    if message == "post locale must be `zh_cn` or `en_us`"
            ));
        }
    }

    #[test]
    fn rejects_markdown_mixed_with_structured_elements() {
        assert!(matches!(
            PostContent::new(PostDocument::new().paragraph([
                PostElement::markdown("**status**"),
                PostElement::text(" ready"),
            ])),
            Err(Error::Validation(message))
                if message == "post markdown elements must occupy their own paragraph"
        ));
    }

    #[test]
    fn deserialization_enforces_post_content_invariants() {
        let invalid_values = [
            json!({}),
            json!({ "": { "content": [[{ "tag": "text", "text": "hello" }]] } }),
            json!({ "ja_jp": { "content": [[{ "tag": "text", "text": "hello" }]] } }),
            json!({ "zh_cn": { "content": [] } }),
            json!({ "zh_cn": { "content": [[]] } }),
            json!({
                "zh_cn": {
                    "content": [[
                        { "tag": "md", "text": "**status**" },
                        { "tag": "text", "text": " ready" }
                    ]]
                }
            }),
            json!({
                "zh_cn": {
                    "content": [[{ "tag": "unsupported", "text": "hello" }]]
                }
            }),
            json!({ "zh_cn": { "content": [[{ "tag": "text" }]] } }),
            json!({
                "zh_cn": {
                    "content": [[{ "tag": "text", "text": "hello", "un_escape": "yes" }]]
                }
            }),
            json!({
                "zh_cn": {
                    "content": [[{ "tag": "text", "text": "hello", "un_escape": null }]]
                }
            }),
            json!({
                "zh_cn": {
                    "content": [[{ "tag": "a", "text": "docs", "href": "https://open.feishu.cn", "un_escape": true }]]
                }
            }),
            json!({
                "zh_cn": {
                    "content": [[{ "tag": "a", "text": "docs", "href": "not a URL" }]]
                }
            }),
            json!({
                "zh_cn": {
                    "content": [[{ "tag": "at", "user_id": "invalid id" }]]
                }
            }),
            json!({
                "zh_cn": {
                    "content": [[{ "tag": "md", "text": "**status**", "style": ["bold"] }]]
                }
            }),
        ];

        for value in invalid_values {
            assert!(serde_json::from_value::<PostContent>(value).is_err());
        }
    }

    #[test]
    fn valid_post_content_round_trips_through_json() {
        let post = PostContent::builder()
            .title("Status")
            .markdown("**ready**")
            .paragraph([
                PostElement::mention("ou_alice").expect("mention"),
                PostElement::text(" read ")
                    .with_unescape(true)
                    .expect("unescape"),
                PostElement::link("the docs", "https://open.feishu.cn")
                    .expect("link")
                    .with_styles([PostStyle::Italic])
                    .expect("styles"),
            ])
            .build()
            .expect("post content");

        let value = serde_json::to_value(&post).expect("serialize post content");
        assert_eq!(
            serde_json::from_value::<PostContent>(value).expect("deserialize post content"),
            post
        );
    }

    #[test]
    fn markdown_elements_reject_structured_styles() {
        assert!(matches!(
            PostElement::markdown("**bold**").with_styles([PostStyle::Bold]),
            Err(Error::Validation(message))
                if message.contains("only supported for text, link, and mention")
        ));
    }

    #[test]
    fn unescape_is_only_available_as_a_boolean_on_text_elements() {
        assert_eq!(
            serde_json::to_value(
                PostElement::text("hello&nbsp;world")
                    .with_unescape(true)
                    .expect("text unescape")
            )
            .expect("serialize text element"),
            json!({
                "tag": "text",
                "text": "hello&nbsp;world",
                "un_escape": true
            })
        );
        assert!(matches!(
            PostElement::markdown("hello").with_unescape(true),
            Err(Error::Validation(message))
                if message == "post un_escape is only supported for text elements"
        ));
    }

    #[test]
    fn standalone_element_deserialization_rejects_invalid_and_extra_fields() {
        for value in [
            json!({ "tag": "text", "text": "hello", "un_escape": "yes" }),
            json!({ "tag": "text", "text": "hello", "un_escape": null }),
            json!({ "tag": "a", "text": "docs", "href": "not a URL" }),
            json!({ "tag": "at", "user_id": "invalid id" }),
            json!({ "tag": "md", "text": "hello", "href": 123 }),
            json!({ "tag": "text", "text": "hello", "unknown": true }),
        ] {
            assert!(serde_json::from_value::<PostElement>(value).is_err());
        }
    }

    #[test]
    fn inserts_multiple_locales() {
        let mut post = PostContent::text("你好");
        post.insert_locale(
            "en_us",
            PostDocument::new().paragraph([PostElement::text("hello")]),
        )
        .expect("English locale");

        assert_eq!(post.locales().len(), 2);
        assert_eq!(
            post.document("en_us").expect("document").paragraphs().len(),
            1
        );

        assert!(matches!(
            post.insert_locale(
                "ja_jp",
                PostDocument::new().paragraph([PostElement::text("こんにちは")]),
            ),
            Err(Error::Validation(message))
                if message == "post locale must be `zh_cn` or `en_us`"
        ));
        assert_eq!(post.locales().len(), 2);
    }
}
