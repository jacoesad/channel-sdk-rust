use crate::card::CardStreamingConfig;
use crate::lark_openapi::OpenApiTransport;
use crate::validation::validate_path_identifier;
use crate::{CardId, Error, MessageId, Recipient, Result};

use super::continuation::{
    ContinuingMarkdownStream, DEFAULT_CONTINUATION_MAX_PAGE_CHARS, validate_continuation_limit,
};
use super::stream::MarkdownStream;
use super::{MarkdownStreamCardProfile, build_streaming_card, validate_stream_content_states};
use crate::message::sender::{MessageSender, resolve_uuid};

#[derive(Debug, Clone)]
pub(super) enum MarkdownStreamTarget {
    Message(Recipient),
    Reply(MessageId),
}

#[derive(Debug, Clone)]
pub(super) struct MarkdownStreamTemplate {
    target: MarkdownStreamTarget,
    profile: MarkdownStreamCardProfile,
    reply_in_thread: Option<bool>,
    max_attempts: usize,
}

impl MarkdownStreamTemplate {
    pub(super) fn follow_up_builder<'a, T>(
        &self,
        sender: &'a MessageSender<T>,
    ) -> MarkdownStreamBuilder<'a, T> {
        MarkdownStreamBuilder {
            sender,
            target: self.target.clone(),
            profile: self.profile.clone(),
            uuid: None,
            reply_in_thread: self.reply_in_thread,
            max_attempts: Some(self.max_attempts),
            continuation_max_page_chars: DEFAULT_CONTINUATION_MAX_PAGE_CHARS,
            card_creation_outcome_unknown: false,
            prepared: None,
            started: false,
        }
    }

    pub(super) fn profile(&self) -> &MarkdownStreamCardProfile {
        &self.profile
    }
}

#[derive(Debug, Clone)]
struct PreparedMarkdownStream {
    target: MarkdownStreamTarget,
    profile: MarkdownStreamCardProfile,
    card_id: CardId,
    message_uuid: String,
    reply_in_thread: Option<bool>,
    max_attempts: usize,
}

/// Builder for starting a high-level Markdown CardKit stream.
///
/// The stream owns CardKit entity creation, message or reply delivery, update
/// sequencing, update idempotency keys, and finalization. Every `append` or
/// `set_content` call on the returned [`MarkdownStream`] performs an update
/// immediately. Call [`MarkdownStream::throttle`] to coalesce generated content
/// without changing the builder lifecycle.
#[derive(Debug)]
pub struct MarkdownStreamBuilder<'a, T> {
    sender: &'a MessageSender<T>,
    target: MarkdownStreamTarget,
    profile: MarkdownStreamCardProfile,
    uuid: Option<String>,
    reply_in_thread: Option<bool>,
    max_attempts: Option<usize>,
    continuation_max_page_chars: usize,
    card_creation_outcome_unknown: bool,
    prepared: Option<PreparedMarkdownStream>,
    started: bool,
}

impl<T> MessageSender<T>
where
    T: OpenApiTransport,
{
    /// Starts building a Markdown stream sent to a chat or user.
    pub fn markdown_stream_message(&self, recipient: Recipient) -> MarkdownStreamBuilder<'_, T> {
        MarkdownStreamBuilder::new(self, MarkdownStreamTarget::Message(recipient))
    }

    /// Starts building a Markdown stream that replies to an existing message.
    pub fn markdown_stream_reply(
        &self,
        parent_message_id: MessageId,
    ) -> MarkdownStreamBuilder<'_, T> {
        MarkdownStreamBuilder::new(self, MarkdownStreamTarget::Reply(parent_message_id))
    }
}

impl<'a, T> MarkdownStreamBuilder<'a, T>
where
    T: OpenApiTransport,
{
    fn new(sender: &'a MessageSender<T>, target: MarkdownStreamTarget) -> Self {
        Self {
            sender,
            target,
            profile: MarkdownStreamCardProfile::default(),
            uuid: None,
            reply_in_thread: None,
            max_attempts: None,
            continuation_max_page_chars: DEFAULT_CONTINUATION_MAX_PAGE_CHARS,
            card_creation_outcome_unknown: false,
            prepared: None,
            started: false,
        }
    }

    /// Sets the placeholder shown before the first generated content update.
    pub fn initial_text(mut self, initial_text: impl Into<String>) -> Self {
        self.profile.initial_text = initial_text.into();
        self
    }

    /// Sets the content used when the stream is finished without output.
    pub fn empty_text(mut self, empty_text: impl Into<String>) -> Self {
        self.profile.empty_text = empty_text.into();
        self
    }

    /// Sets the preview summary shown while generation is active.
    pub fn streaming_summary(mut self, summary: impl Into<String>) -> Self {
        self.profile.streaming_summary = summary.into();
        self
    }

    /// Overrides the final preview summary.
    ///
    /// Without an override, the stream derives a compact summary from its
    /// final Markdown content.
    pub fn final_summary(mut self, summary: impl Into<String>) -> Self {
        self.profile.final_summary = Some(summary.into());
        self
    }

    /// Sets the CardKit client-side typewriter configuration.
    pub fn streaming_config(mut self, config: CardStreamingConfig) -> Self {
        self.profile.streaming_config = config;
        self
    }

    /// Sets the idempotency key used to send the card reference message.
    ///
    /// Element and settings updates receive separate managed keys that remain
    /// stable across each operation's internal transport retries.
    pub fn uuid(mut self, uuid: impl Into<String>) -> Self {
        self.uuid = Some(uuid.into());
        self
    }

    /// Requests thread placement for a stream created as a reply.
    ///
    /// Calling this on a stream created by `markdown_stream_message` is a
    /// validation error when `start` runs.
    pub fn reply_in_thread(mut self, reply_in_thread: bool) -> Self {
        self.reply_in_thread = Some(reply_in_thread);
        self
    }

    /// Sets the maximum number of transport attempts for message delivery and
    /// each CardKit update. Values below `1` are clamped to `1`.
    ///
    /// Card entity creation is not retried because that API does not expose an
    /// idempotency key.
    pub fn max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = Some(max_attempts.max(1));
        self
    }

    /// Sets the soft Unicode character limit for each continuation card.
    ///
    /// The controller may split earlier to stay within CardKit's serialized
    /// card-size limit. This setting only affects [`Self::start_continuing`].
    pub fn continuation_max_page_chars(mut self, max_page_chars: usize) -> Self {
        self.continuation_max_page_chars = max_page_chars;
        self
    }

    /// Returns the prepared CardKit entity after the first successful create.
    ///
    /// When message delivery has an ambiguous transport, HTTP-status, or
    /// response-decoding outcome, keep this builder and retry with the same
    /// entrypoint: [`Self::start`] or [`Self::start_continuing`]. The retry
    /// reuses this card and the original message idempotency key instead of
    /// creating another entity.
    pub fn prepared_card_id(&self) -> Option<&CardId> {
        self.prepared.as_ref().map(|prepared| &prepared.card_id)
    }

    pub(super) fn has_unknown_card_creation_outcome(&self) -> bool {
        self.card_creation_outcome_unknown
    }

    /// Validates known complete content without performing remote operations.
    ///
    /// Call this before [`Self::start`] when the complete output is already
    /// available. It checks both the active streaming card and the closed card
    /// state against the element and whole-card limits.
    pub fn preflight_content(&self, content: impl AsRef<str>) -> Result<()> {
        let profile = self
            .prepared
            .as_ref()
            .map(|prepared| &prepared.profile)
            .unwrap_or(&self.profile);
        validate_stream_content_states(content.as_ref(), profile).map(|_| ())
    }

    /// Creates and sends the streaming card, returning its active controller.
    ///
    /// This method keeps a successfully created card entity inside the builder
    /// until delivery is acknowledged. Bind the builder to a mutable variable
    /// when the caller needs to retry an ambiguous message-delivery outcome
    /// from this method. Card creation itself has no idempotency key: after an
    /// ambiguous creation failure or cancellation, later calls return a
    /// validation error instead of creating another entity.
    pub async fn start(&mut self) -> Result<MarkdownStream<'a, T>> {
        if self.started {
            return Err(Error::Validation(
                "markdown stream builder has already started a stream".to_owned(),
            ));
        }

        if self.prepared.is_none() {
            if self.card_creation_outcome_unknown {
                return Err(Error::Validation(
                    "CardKit entity creation has an unknown outcome and cannot be retried safely"
                        .to_owned(),
                ));
            }

            let message_uuid = resolve_uuid(self.uuid.clone())?;
            validate_builder_target(&self.target, self.reply_in_thread)?;
            let initial_card = build_streaming_card(
                &self.profile.initial_text,
                true,
                &self.profile.streaming_summary,
                &self.profile.streaming_config,
            )?;
            validate_stream_content_states(&self.profile.empty_text, &self.profile)?;

            let client = self.sender.client();
            let request = client.prepare_card_entity_create(&initial_card)?;
            let token = client.tenant_access_token().await?;
            let request = request.with_bearer_auth(token);
            // Only the actual non-idempotent request crosses the ambiguous
            // boundary. Authentication and request preparation remain safely
            // retryable because they cannot create remote CardKit state.
            self.card_creation_outcome_unknown = true;
            let card_id = match client.send_prepared_card_entity_create(request).await {
                Ok(card_id) => card_id,
                Err(error) => {
                    if matches!(&error, Error::Api { .. }) {
                        self.card_creation_outcome_unknown = false;
                    }
                    return Err(error);
                }
            };
            self.prepared = Some(PreparedMarkdownStream {
                target: self.target.clone(),
                profile: self.profile.clone(),
                card_id,
                message_uuid,
                reply_in_thread: self.reply_in_thread,
                max_attempts: self.sender.max_attempts(self.max_attempts),
            });
            self.card_creation_outcome_unknown = false;
        }

        let prepared = self
            .prepared
            .as_ref()
            .expect("prepared stream must exist after successful creation")
            .clone();
        let message_id = match prepared.target.clone() {
            MarkdownStreamTarget::Message(recipient) => {
                self.sender
                    .card_reference_message(recipient, prepared.card_id.clone())
                    .uuid(prepared.message_uuid.clone())
                    .max_attempts(prepared.max_attempts)
                    .send()
                    .await?
            }
            MarkdownStreamTarget::Reply(parent_message_id) => {
                let mut builder = self
                    .sender
                    .card_reference_reply(parent_message_id, prepared.card_id.clone())
                    .uuid(prepared.message_uuid.clone())
                    .max_attempts(prepared.max_attempts);
                if let Some(reply_in_thread) = prepared.reply_in_thread {
                    builder = builder.reply_in_thread(reply_in_thread);
                }
                builder.send().await?
            }
        };

        self.started = true;

        Ok(MarkdownStream::new(
            self.sender,
            prepared.card_id,
            message_id,
            prepared.profile,
            prepared.max_attempts,
        ))
    }

    /// Creates and sends a stream that rolls oversized Markdown onto new cards.
    ///
    /// Follow-up cards use fresh managed message UUIDs and independent CardKit
    /// update sequences. The returned controller preserves the complete source
    /// Markdown while exposing every created card and message through its page
    /// list. It prefers natural text boundaries but otherwise treats Markdown
    /// as plain source: constructs may span pages and are not rewritten.
    /// If initial message delivery has an ambiguous outcome, call this method
    /// again on the same mutable builder to preserve continuation behavior.
    pub async fn start_continuing(&mut self) -> Result<ContinuingMarkdownStream<'a, T>> {
        validate_continuation_limit(self.continuation_max_page_chars)?;
        let template = self.continuation_template();
        let max_page_chars = self.continuation_max_page_chars;
        let stream = self.start().await?;
        Ok(ContinuingMarkdownStream::new(
            self.sender,
            stream,
            template,
            max_page_chars,
        ))
    }

    fn continuation_template(&self) -> MarkdownStreamTemplate {
        match &self.prepared {
            Some(prepared) => MarkdownStreamTemplate {
                target: prepared.target.clone(),
                profile: prepared.profile.clone(),
                reply_in_thread: prepared.reply_in_thread,
                max_attempts: prepared.max_attempts,
            },
            None => MarkdownStreamTemplate {
                target: self.target.clone(),
                profile: self.profile.clone(),
                reply_in_thread: self.reply_in_thread,
                max_attempts: self.sender.max_attempts(self.max_attempts),
            },
        }
    }
}

fn validate_builder_target(
    target: &MarkdownStreamTarget,
    reply_in_thread: Option<bool>,
) -> Result<()> {
    match target {
        MarkdownStreamTarget::Message(recipient) => {
            if reply_in_thread.is_some() {
                return Err(Error::Validation(
                    "reply_in_thread is only supported for markdown stream replies".to_owned(),
                ));
            }
            match recipient {
                Recipient::Chat(chat_id) => validate_path_identifier(chat_id, "chat_id")?,
                Recipient::User(open_id) => validate_path_identifier(open_id, "open_id")?,
            }
        }
        MarkdownStreamTarget::Reply(message_id) => {
            validate_path_identifier(&message_id.0, "message_id")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelConfig;
    use crate::message::streaming::test_support::{
        FakeResponse, FakeTransport, assert_pending_once, block_on, card_response,
        message_response, token_response,
    };

    #[test]
    fn start_does_not_repeat_ambiguous_card_creation() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Err(Error::Transport("card response lost".to_owned())),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .max_attempts(1);

        let error = block_on(builder.start()).expect_err("card creation outcome is unknown");
        assert!(matches!(error, Error::Transport(_)));
        let error = block_on(builder.start()).expect_err("ambiguous create cannot be repeated");
        assert!(matches!(error, Error::Validation(message) if message.contains("unknown outcome")));
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn cancelled_card_creation_cannot_be_repeated() {
        let short_lived_token = crate::lark_openapi::HttpResponse::json(
            200,
            serde_json::json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "short-lived-token",
                "expire": 1
            }),
        );
        let transport = FakeTransport::scripted(vec![
            FakeResponse::ready(short_lived_token),
            FakeResponse::pending_once(card_response("7355372766134157317")),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .max_attempts(1);

        assert_pending_once(builder.start());
        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[1].url.path().ends_with("/cardkit/v1/cards"));

        let error = block_on(builder.start()).expect_err("cancelled create cannot be repeated");
        assert!(matches!(error, Error::Validation(message) if message.contains("unknown outcome")));
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn http_status_during_card_creation_cannot_be_retried() {
        let transport = FakeTransport::http(vec![
            token_response(),
            crate::lark_openapi::HttpResponse::json(
                502,
                serde_json::json!({ "message": "bad gateway" }),
            ),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .max_attempts(1);

        let error = block_on(builder.start()).expect_err("HTTP outcome is ambiguous");
        assert!(matches!(error, Error::HttpStatus { status: 502 }));
        let error = block_on(builder.start()).expect_err("ambiguous create cannot be repeated");
        assert!(matches!(error, Error::Validation(message) if message.contains("unknown outcome")));
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn response_decoding_during_card_creation_cannot_be_retried() {
        let transport =
            FakeTransport::new(vec![Ok(token_response()), Err(response_decoding_error())]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .max_attempts(1);

        let error = block_on(builder.start()).expect_err("decoding outcome is ambiguous");
        assert!(matches!(error, Error::Serde(_)));
        let error = block_on(builder.start()).expect_err("ambiguous create cannot be repeated");
        assert!(matches!(error, Error::Validation(message) if message.contains("unknown outcome")));
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn cancelled_token_request_remains_retryable_before_card_creation() {
        let transport = FakeTransport::scripted(vec![
            FakeResponse::pending_once(token_response()),
            FakeResponse::ready(token_response()),
            FakeResponse::ready(card_response("7355372766134157315")),
            FakeResponse::ready(message_response("om_after_auth_retry")),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .max_attempts(1);

        assert_pending_once(builder.start());
        let stream = block_on(builder.start()).expect("token cancellation precedes remote create");

        assert_eq!(stream.card_id().as_str(), "7355372766134157315");
        assert_eq!(transport.calls().len(), 4);
    }

    #[test]
    fn definitive_card_creation_error_can_be_retried() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Err(Error::Api {
                code: 230099,
                message: "rejected".to_owned(),
            }),
            Ok(card_response("7355372766134157316")),
            Ok(message_response("om_retried")),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .max_attempts(1);

        let error = block_on(builder.start()).expect_err("definitive API error");
        assert!(matches!(error, Error::Api { code: 230099, .. }));
        let stream = block_on(builder.start()).expect("retry after definitive rejection");

        assert_eq!(stream.card_id().as_str(), "7355372766134157316");
        assert_eq!(transport.calls().len(), 4);
    }

    #[test]
    fn start_reuses_prepared_card_and_message_uuid_after_delivery_transport_error() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157318")),
            Err(Error::Transport("message response lost".to_owned())),
            Ok(message_response("om_recovered")),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .uuid("stream-start-1")
            .max_attempts(1);

        let error = block_on(builder.start()).expect_err("ambiguous delivery failure");
        assert!(matches!(error, Error::Transport(_)));
        assert_eq!(
            builder.prepared_card_id().map(crate::CardId::as_str),
            Some("7355372766134157318")
        );

        let stream = block_on(builder.start()).expect("recovered prepared stream");
        assert_eq!(stream.card_id().as_str(), "7355372766134157318");
        assert_eq!(stream.message_id().0, "om_recovered");

        let calls = transport.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[2].body["uuid"], "stream-start-1");
        assert_eq!(calls[3].body["uuid"], "stream-start-1");
        assert_eq!(calls[2].body["content"], calls[3].body["content"]);

        let error = block_on(builder.start()).expect_err("builder starts only once");
        assert!(matches!(error, Error::Validation(_)));
    }

    #[test]
    fn start_continuing_reuses_prepared_card_after_delivery_decoding_error() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157319")),
            Err(response_decoding_error()),
            Ok(message_response("om_continuing_recovered")),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .uuid("stream-continuing-1")
            .max_attempts(1);

        let error = block_on(builder.start_continuing()).expect_err("ambiguous delivery failure");
        assert!(matches!(error, Error::Serde(_)));
        let stream = block_on(builder.start_continuing()).expect("recovered continuing stream");

        assert_eq!(stream.pages().len(), 1);
        assert_eq!(stream.current_card_id().as_str(), "7355372766134157319");
        assert_eq!(stream.current_message_id().0, "om_continuing_recovered");
        let calls = transport.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[2].body["uuid"], "stream-continuing-1");
        assert_eq!(calls[3].body["uuid"], "stream-continuing-1");
        assert_eq!(calls[2].body["content"], calls[3].body["content"]);
    }

    #[test]
    fn prepared_reply_reuses_the_original_thread_option() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157321")),
            Err(Error::Transport("reply response lost".to_owned())),
            Ok(message_response("om_reply_recovered")),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut builder = sender
            .markdown_stream_reply(crate::MessageId("om_parent".to_owned()))
            .reply_in_thread(true)
            .max_attempts(1);

        let error = block_on(builder.start()).expect_err("ambiguous reply failure");
        assert!(matches!(error, Error::Transport(_)));

        builder = builder.reply_in_thread(false);
        let stream = block_on(builder.start()).expect("recovered prepared reply");
        assert_eq!(stream.message_id().0, "om_reply_recovered");

        let calls = transport.calls();
        assert_eq!(calls[2].body["reply_in_thread"], true);
        assert_eq!(calls[3].body["reply_in_thread"], true);
        assert_eq!(calls[2].body["uuid"], calls[3].body["uuid"]);
        assert_eq!(calls[2].body["content"], calls[3].body["content"]);
    }

    #[test]
    fn validation_fails_before_creating_remote_state() {
        let transport = FakeTransport::http(vec![]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);

        let error = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .uuid("")
                .start(),
        )
        .expect_err("invalid message UUID");
        assert!(matches!(error, Error::Validation(_)));

        let error = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .reply_in_thread(true)
                .start(),
        )
        .expect_err("thread option on a new message");
        assert!(matches!(error, Error::Validation(_)));

        let error = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .initial_text("")
                .start(),
        )
        .expect_err("empty placeholder");
        assert!(matches!(error, Error::Validation(_)));

        let error = block_on(
            sender
                .markdown_stream_message(Recipient::Chat(String::new()))
                .start(),
        )
        .expect_err("empty chat ID");
        assert!(matches!(error, Error::Validation(_)));

        let error = block_on(
            sender
                .markdown_stream_reply(crate::MessageId("invalid/message".to_owned()))
                .start(),
        )
        .expect_err("invalid parent message ID");
        assert!(matches!(error, Error::Validation(_)));
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn preflight_rejects_known_oversized_content_without_remote_calls() {
        let transport = FakeTransport::http(vec![]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let builder = sender.markdown_stream_message(Recipient::Chat("oc_123".to_owned()));

        builder
            .preflight_content("valid content")
            .expect("valid content preflight");
        let error = builder
            .preflight_content("界".repeat(100_001))
            .expect_err("element character limit");
        assert!(matches!(error, Error::Validation(_)));
        let error = builder
            .preflight_content("界".repeat(11_000))
            .expect_err("whole card byte limit");
        assert!(matches!(error, Error::Validation(_)));
        assert!(transport.calls().is_empty());
    }

    fn response_decoding_error() -> Error {
        serde_json::from_str::<serde_json::Value>("{")
            .expect_err("invalid JSON")
            .into()
    }
}
