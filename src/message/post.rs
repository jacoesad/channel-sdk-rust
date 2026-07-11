use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::{Map, Value};
use url::Url;

use crate::{Error, Result};

const DEFAULT_POST_LOCALE: &str = "zh_cn";
const ENGLISH_POST_LOCALE: &str = "en_us";

/// Lark/Feishu rich-text `post` message content.
///
/// The serialized shape is a locale map such as
/// `{ "zh_cn": { "title": "", "content": [[...]] } }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PostContent {
    locales: BTreeMap<String, PostDocument>,
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PostDocument {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    title: String,
    content: Vec<Vec<PostElement>>,
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
#[derive(Debug, Clone)]
pub struct PostContentBuilder {
    locale: String,
    document: PostDocument,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PostElement(Value);

impl PostElement {
    /// Creates a plain text element.
    pub fn text(text: impl Into<String>) -> Self {
        Self(object([
            ("tag", Value::String("text".to_owned())),
            ("text", Value::String(text.into())),
        ]))
    }

    /// Creates a validated hyperlink element.
    pub fn link(text: impl Into<String>, href: impl Into<String>) -> Result<Self> {
        let href = href.into();
        Url::parse(&href)?;
        Ok(Self(object([
            ("tag", Value::String("a".to_owned())),
            ("text", Value::String(text.into())),
            ("href", Value::String(href)),
        ])))
    }

    /// Creates an @ mention for an open_id, user_id, or union_id.
    pub fn mention(user_id: impl Into<String>) -> Result<Self> {
        let user_id = validate_mention_id(user_id.into())?;
        Ok(Self(object([
            ("tag", Value::String("at".to_owned())),
            ("user_id", Value::String(user_id)),
        ])))
    }

    /// Creates an @all mention.
    pub fn mention_all() -> Self {
        Self(object([
            ("tag", Value::String("at".to_owned())),
            ("user_id", Value::String("all".to_owned())),
        ]))
    }

    /// Creates a native Markdown element.
    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self(object([
            ("tag", Value::String("md".to_owned())),
            ("text", Value::String(markdown.into())),
        ]))
    }

    /// Sets the official `un_escape` flag on a plain text element.
    pub fn with_unescape(mut self, unescape: bool) -> Result<Self> {
        let Some(element) = self.0.as_object_mut() else {
            return Err(Error::Validation(
                "post element must be a JSON object".to_owned(),
            ));
        };
        let is_text = element
            .get("tag")
            .and_then(Value::as_str)
            .is_some_and(|tag| tag == "text");
        if !is_text {
            return Err(Error::Validation(
                "post un_escape is only supported for text elements".to_owned(),
            ));
        }

        element.insert("un_escape".to_owned(), Value::Bool(unescape));
        Ok(self)
    }

    /// Applies supported text styles to text, link, or mention elements.
    pub fn with_styles(mut self, styles: impl IntoIterator<Item = PostStyle>) -> Result<Self> {
        let Some(element) = self.0.as_object_mut() else {
            return Err(Error::Validation(
                "post element must be a JSON object".to_owned(),
            ));
        };
        let supports_style = element
            .get("tag")
            .and_then(Value::as_str)
            .is_some_and(|tag| matches!(tag, "text" | "a" | "at"));
        if !supports_style {
            return Err(Error::Validation(
                "post styles are only supported for text, link, and mention elements".to_owned(),
            ));
        }

        let styles = styles
            .into_iter()
            .map(|style| Value::String(style.as_str().to_owned()))
            .collect::<Vec<_>>();
        if !styles.is_empty() {
            element.insert("style".to_owned(), Value::Array(styles));
        }
        Ok(self)
    }

    /// Returns the underlying official element JSON.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// Consumes the helper and returns the underlying official element JSON.
    pub fn into_value(self) -> Value {
        self.0
    }

    fn validate(&self) -> Result<&str> {
        let element = self
            .0
            .as_object()
            .ok_or_else(|| Error::Validation("post element must be a JSON object".to_owned()))?;
        let tag = required_string(element, "tag", "post element")?;

        if tag != "text" && element.contains_key("un_escape") {
            return Err(Error::Validation(
                "post un_escape is only supported for text elements".to_owned(),
            ));
        }

        match tag {
            "text" => {
                required_string(element, "text", "post text element")?;
                if element
                    .get("un_escape")
                    .is_some_and(|unescape| !unescape.is_boolean())
                {
                    return Err(Error::Validation(
                        "post text element `un_escape` must be a boolean".to_owned(),
                    ));
                }
                validate_styles(element.get("style"))?;
            }
            "a" => {
                required_string(element, "text", "post link element")?;
                Url::parse(required_string(element, "href", "post link element")?)?;
                validate_styles(element.get("style"))?;
            }
            "at" => {
                let user_id = required_string(element, "user_id", "post mention element")?;
                validate_mention_id(user_id.to_owned())?;
                validate_styles(element.get("style"))?;
            }
            "md" => {
                required_string(element, "text", "post markdown element")?;
                if element.contains_key("style") {
                    return Err(Error::Validation(
                        "post markdown elements do not support styles".to_owned(),
                    ));
                }
            }
            _ => {
                return Err(Error::Validation(format!(
                    "unsupported typed post element tag: {tag}"
                )));
            }
        }

        Ok(tag)
    }
}

/// Text styles supported by Lark/Feishu structured post elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostStyle {
    Bold,
    Underline,
    LineThrough,
    Italic,
}

impl PostStyle {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Underline => "underline",
            Self::LineThrough => "lineThrough",
            Self::Italic => "italic",
        }
    }
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

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    context: &str,
) -> Result<&'a str> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        Error::Validation(format!("{context} must contain a string `{field}` field"))
    })
}

fn validate_styles(styles: Option<&Value>) -> Result<()> {
    let Some(styles) = styles else {
        return Ok(());
    };
    let styles = styles
        .as_array()
        .ok_or_else(|| Error::Validation("post element `style` must be an array".to_owned()))?;
    if styles.iter().all(|style| {
        style
            .as_str()
            .is_some_and(|style| matches!(style, "bold" | "underline" | "lineThrough" | "italic"))
    }) {
        Ok(())
    } else {
        Err(Error::Validation(
            "post element contains an unsupported style".to_owned(),
        ))
    }
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

fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<Map<_, _>>(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

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
            PostElement::text("hello&nbsp;world")
                .with_unescape(true)
                .expect("text unescape")
                .into_value(),
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
