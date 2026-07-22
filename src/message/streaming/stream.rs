use std::time::Duration;

use crate::card::{CardElementContent, CardSettings};
use crate::lark_openapi::{CardUpdateOptions, OpenApiTransport};
use crate::{CardId, Error, MessageId, Result};

use super::super::sender::{MessageSender, generate_idempotency_key};
use super::throttle::ThrottledMarkdownStream;
use super::{
    MarkdownStreamCardProfile, STREAM_ELEMENT_ID, derive_summary, validate_stream_content_states,
};

#[derive(Debug, Clone)]
enum PendingMarkdownStreamOperation {
    Content {
        content: CardElementContent,
        options: CardUpdateOptions,
    },
    Finish {
        settings: CardSettings,
        options: CardUpdateOptions,
    },
}

/// Active high-level Markdown stream backed by one CardKit entity.
///
/// Call `append` with token deltas or `set_content` with a complete snapshot,
/// then call `finish` to disable CardKit streaming mode. Dropping an unfinished
/// stream cannot perform asynchronous cleanup; Lark/Feishu closes the card
/// automatically after its platform timeout.
#[derive(Debug)]
pub struct MarkdownStream<'a, T> {
    sender: &'a MessageSender<T>,
    card_id: CardId,
    message_id: MessageId,
    profile: MarkdownStreamCardProfile,
    content: Option<CardElementContent>,
    next_sequence: u32,
    max_attempts: usize,
    finished: bool,
    pending: Option<PendingMarkdownStreamOperation>,
}

impl<'a, T> MarkdownStream<'a, T> {
    pub(super) fn new(
        sender: &'a MessageSender<T>,
        card_id: CardId,
        message_id: MessageId,
        profile: MarkdownStreamCardProfile,
        max_attempts: usize,
    ) -> Self {
        Self {
            sender,
            card_id,
            message_id,
            profile,
            content: None,
            next_sequence: 1,
            max_attempts,
            finished: false,
            pending: None,
        }
    }
}

impl<'a, T> MarkdownStream<'a, T>
where
    T: OpenApiTransport,
{
    /// Returns the message that displays this stream.
    pub fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    /// Returns the CardKit entity updated by this stream.
    pub fn card_id(&self) -> &CardId {
        &self.card_id
    }

    /// Returns the accumulated generated content, excluding the placeholder.
    pub fn content(&self) -> Option<&str> {
        self.content.as_ref().map(CardElementContent::as_str)
    }

    /// Returns the sequence that will be used by the next CardKit operation.
    pub fn next_sequence(&self) -> u32 {
        self.next_sequence
    }

    /// Returns whether `finish` completed successfully.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Returns whether an ambiguous remote failure left one operation awaiting
    /// an idempotent replay.
    pub fn has_pending_operation(&self) -> bool {
        self.pending.is_some()
    }

    /// Wraps this stream in a runtime-independent content update throttler.
    ///
    /// The first content update is sent immediately. Later updates that arrive
    /// before `min_update_interval` elapses are coalesced until another update
    /// reaches the interval or the caller invokes `flush` or `finish`.
    pub fn throttle(self, min_update_interval: Duration) -> ThrottledMarkdownStream<'a, T> {
        ThrottledMarkdownStream::new(self, min_update_interval)
    }

    /// Appends one delta verbatim and immediately pushes the accumulated text.
    ///
    /// Empty chunks are a no-op. This method does not merge overlapping text;
    /// producers that already hold a complete snapshot should use
    /// `set_content` instead.
    pub async fn append(&mut self, chunk: impl AsRef<str>) -> Result<()> {
        self.ensure_active()?;
        let chunk = chunk.as_ref();
        if chunk.is_empty() {
            return Ok(());
        }

        let mut content = self.content().unwrap_or_default().to_owned();
        content.push_str(chunk);
        self.update_content(content).await
    }

    /// Replaces the accumulated text with a complete snapshot and pushes it.
    pub async fn set_content(&mut self, content: impl Into<String>) -> Result<()> {
        self.ensure_active()?;
        self.update_content(content.into()).await
    }

    /// Retries the operation retained after an ambiguous remote failure.
    ///
    /// The original payload, sequence, and UUID are reused. Validation and API
    /// errors are definitive and clear the retained operation. Transport, HTTP
    /// status, and response-decoding failures preserve the operation because
    /// the remote update may already have been applied.
    pub async fn retry_pending(&mut self) -> Result<()> {
        let Some(operation) = self.pending.clone() else {
            return Ok(());
        };

        let result = match &operation {
            PendingMarkdownStreamOperation::Content { content, options } => {
                self.sender
                    .retry_transport_errors(self.max_attempts, || {
                        self.sender.client().update_card_element_content(
                            &self.card_id,
                            STREAM_ELEMENT_ID,
                            content.as_str(),
                            options.clone(),
                        )
                    })
                    .await
            }
            PendingMarkdownStreamOperation::Finish { settings, options } => {
                self.sender
                    .retry_transport_errors(self.max_attempts, || {
                        self.sender.client().update_card_settings(
                            &self.card_id,
                            settings,
                            options.clone(),
                        )
                    })
                    .await
            }
        };

        if let Err(error) = result {
            if !operation_outcome_may_be_ambiguous(&error) {
                self.pending = None;
            }
            return Err(error);
        }

        match operation {
            PendingMarkdownStreamOperation::Content { content, options } => {
                self.content = Some(content);
                self.next_sequence = options.sequence.saturating_add(1);
            }
            PendingMarkdownStreamOperation::Finish { options, .. } => {
                self.next_sequence = options.sequence.saturating_add(1);
                self.finished = true;
            }
        }
        self.pending = None;
        Ok(())
    }

    /// Disables CardKit streaming mode and updates the final preview summary.
    ///
    /// Finishing an already completed local stream is an idempotent no-op. If
    /// no content was produced, the configured empty text is pushed first.
    pub async fn finish(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        if self.pending.is_some() {
            self.retry_pending().await?;
            if self.finished {
                return Ok(());
            }
        }
        if self.content.is_none() {
            self.set_content(self.profile.empty_text.clone()).await?;
        }

        let sequence = self.next_sequence;
        let uuid = generate_idempotency_key();
        let summary = self.final_summary();
        let settings = CardSettings::new().streaming_mode(false).summary(summary);
        let options = CardUpdateOptions::new(sequence).uuid(uuid);
        self.pending = Some(PendingMarkdownStreamOperation::Finish { settings, options });
        self.retry_pending().await
    }

    async fn update_content(&mut self, content: String) -> Result<()> {
        if let Some(pending) = &self.pending {
            return match pending {
                PendingMarkdownStreamOperation::Content {
                    content: pending_content,
                    ..
                } if pending_content.as_str() == content => self.retry_pending().await,
                _ => Err(Error::Validation(
                    "retry the pending markdown stream operation before changing content"
                        .to_owned(),
                )),
            };
        }
        if self.content() == Some(content.as_str()) {
            return Ok(());
        }

        let content = validate_stream_content_states(&content, &self.profile)?;
        let sequence = self.next_sequence;
        let uuid = generate_idempotency_key();
        let options = CardUpdateOptions::new(sequence).uuid(uuid);
        self.pending = Some(PendingMarkdownStreamOperation::Content { content, options });
        self.retry_pending().await
    }

    fn ensure_active(&self) -> Result<()> {
        if self.finished {
            return Err(Error::Validation(
                "markdown stream is already finished".to_owned(),
            ));
        }
        Ok(())
    }

    fn final_summary(&self) -> String {
        self.profile.final_summary.clone().unwrap_or_else(|| {
            derive_summary(self.content().unwrap_or(self.profile.empty_text.as_str()))
        })
    }

    pub(super) fn validate_content(&self, content: &str) -> Result<()> {
        validate_stream_content_states(content, &self.profile).map(|_| ())
    }

    pub(super) fn has_pending_finish(&self) -> bool {
        matches!(
            self.pending,
            Some(PendingMarkdownStreamOperation::Finish { .. })
        )
    }

    pub(super) fn pending_content(&self) -> Option<&str> {
        match &self.pending {
            Some(PendingMarkdownStreamOperation::Content { content, .. }) => Some(content.as_str()),
            _ => None,
        }
    }
}

fn operation_outcome_may_be_ambiguous(error: &Error) -> bool {
    matches!(
        error,
        Error::Transport(_) | Error::HttpStatus { .. } | Error::Serde(_)
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::lark_openapi::{HttpMethod, HttpResponse};
    use crate::message::streaming::STREAM_ELEMENT_ID;
    use crate::message::streaming::test_support::{
        FakeTransport, assert_card_content_update, block_on, card_response, message_response,
        ok_response, token_response,
    };
    use crate::{CardStreamingPlatformValues, ChannelConfig, Recipient};

    #[test]
    fn streams_markdown_message_with_accumulated_content_and_sequences() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157313"),
            message_response("om_stream"),
            ok_response(),
            ok_response(),
            ok_response(),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let config =
            crate::CardStreamingConfig::new().print_step(CardStreamingPlatformValues::new(2));

        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .initial_text("H")
                .streaming_summary("Working")
                .streaming_config(config)
                .uuid("stream-message-1")
                .start(),
        )
        .expect("started stream");

        assert_eq!(
            stream.message_id(),
            &crate::MessageId("om_stream".to_owned())
        );
        assert_eq!(stream.card_id().as_str(), "7355372766134157313");
        assert_eq!(stream.content(), None);
        assert_eq!(stream.next_sequence(), 1);

        block_on(stream.append("共 3")).expect("first update");
        block_on(stream.append("3 条")).expect("second update");
        assert_eq!(stream.content(), Some("共 33 条"));
        assert_eq!(stream.next_sequence(), 3);

        let calls_before_duplicate = transport.calls().len();
        block_on(stream.set_content("共 33 条")).expect("duplicate snapshot is a no-op");
        assert_eq!(transport.calls().len(), calls_before_duplicate);

        block_on(stream.finish()).expect("finished stream");
        assert!(stream.is_finished());
        assert_eq!(stream.next_sequence(), 4);

        let calls_before_second_finish = transport.calls().len();
        block_on(stream.finish()).expect("second finish is a no-op");
        assert_eq!(transport.calls().len(), calls_before_second_finish);

        let error = block_on(stream.append("late")).expect_err("finished stream rejects updates");
        assert!(matches!(error, Error::Validation(_)));

        let calls = transport.calls();
        assert_eq!(calls.len(), 6);

        let initial_card: Value = serde_json::from_str(
            calls[1].body["data"]
                .as_str()
                .expect("serialized initial card"),
        )
        .expect("initial card JSON");
        assert_eq!(initial_card["config"]["streaming_mode"], true);
        assert_eq!(initial_card["config"]["summary"]["content"], "Working");
        assert_eq!(
            initial_card["config"]["streaming_config"]["print_step"]["default"],
            2
        );
        assert_eq!(
            initial_card["body"]["elements"][0]["element_id"],
            STREAM_ELEMENT_ID
        );
        assert_eq!(initial_card["body"]["elements"][0]["content"], "H");

        assert_eq!(calls[2].body["uuid"], "stream-message-1");
        let reference: Value = serde_json::from_str(
            calls[2].body["content"]
                .as_str()
                .expect("card reference content"),
        )
        .expect("card reference JSON");
        assert_eq!(reference["data"]["card_id"], "7355372766134157313");

        assert_card_content_update(&calls[3], 1, "共 3");
        assert_card_content_update(&calls[4], 2, "共 33 条");
        assert_eq!(calls[5].method, HttpMethod::Patch);
        assert_eq!(calls[5].body["sequence"], 3);
        let settings: Value = serde_json::from_str(
            calls[5].body["settings"]
                .as_str()
                .expect("serialized settings"),
        )
        .expect("settings JSON");
        assert_eq!(settings["config"]["streaming_mode"], false);
        assert_eq!(settings["config"]["summary"]["content"], "共 33 条");
    }

    #[test]
    fn empty_reply_pushes_fallback_before_finishing_in_thread() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157314"),
            message_response("om_reply"),
            ok_response(),
            ok_response(),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);

        let mut stream = block_on(
            sender
                .markdown_stream_reply(crate::MessageId("om_parent".to_owned()))
                .reply_in_thread(true)
                .empty_text("No response")
                .final_summary("Done")
                .start(),
        )
        .expect("started reply stream");
        block_on(stream.finish()).expect("finished empty stream");

        let calls = transport.calls();
        assert_eq!(
            calls[2].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_parent/reply"
        );
        assert_eq!(calls[2].body["reply_in_thread"], true);
        assert_card_content_update(&calls[3], 1, "No response");
        assert_eq!(calls[4].body["sequence"], 2);
        let settings: Value = serde_json::from_str(
            calls[4].body["settings"]
                .as_str()
                .expect("serialized settings"),
        )
        .expect("settings JSON");
        assert_eq!(settings["config"]["summary"]["content"], "Done");
    }

    #[test]
    fn retries_transport_failures_with_the_same_sequence_and_uuid() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157315")),
            Ok(message_response("om_retry")),
            Err(Error::Transport("temporary network error".to_owned())),
            Ok(ok_response()),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);

        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .max_attempts(2)
                .start(),
        )
        .expect("started stream");
        block_on(stream.append("hello")).expect("retried update");

        let calls = transport.calls();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[3].body["sequence"], 1);
        assert_eq!(calls[4].body["sequence"], 1);
        assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
        assert!(
            calls[3].body["uuid"]
                .as_str()
                .is_some_and(|uuid| { uuid.starts_with("lc-") && uuid.chars().count() <= 64 })
        );
        assert_eq!(stream.content(), Some("hello"));
        assert_eq!(stream.next_sequence(), 2);
    }

    #[test]
    fn pending_content_update_reuses_operation_and_blocks_different_content() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157319")),
            Ok(message_response("om_pending_content")),
            Err(Error::Transport("content response lost".to_owned())),
            Ok(ok_response()),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .max_attempts(1)
                .start(),
        )
        .expect("started stream");

        let error = block_on(stream.append("hello")).expect_err("ambiguous update failure");
        assert!(matches!(error, Error::Transport(_)));
        assert!(stream.has_pending_operation());
        assert_eq!(stream.content(), None);
        assert_eq!(stream.next_sequence(), 1);

        let calls_before_different_content = transport.calls().len();
        let error = block_on(stream.append("world")).expect_err("pending update must be resolved");
        assert!(matches!(error, Error::Validation(_)));
        assert_eq!(transport.calls().len(), calls_before_different_content);
        assert!(stream.has_pending_operation());

        block_on(stream.append("hello")).expect("same logical update is replayed");
        assert!(!stream.has_pending_operation());
        assert_eq!(stream.content(), Some("hello"));
        assert_eq!(stream.next_sequence(), 2);

        let calls = transport.calls();
        assert_eq!(calls[3].body["sequence"], 1);
        assert_eq!(calls[4].body["sequence"], 1);
        assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
        assert_eq!(calls[3].body["content"], calls[4].body["content"]);
    }

    #[test]
    fn pending_finish_reuses_operation_until_acknowledged() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157320")),
            Ok(message_response("om_pending_finish")),
            Ok(ok_response()),
            Err(Error::Transport("finish response lost".to_owned())),
            Ok(ok_response()),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .max_attempts(1)
                .start(),
        )
        .expect("started stream");
        block_on(stream.set_content("done")).expect("content update");

        let error = block_on(stream.finish()).expect_err("ambiguous finish failure");
        assert!(matches!(error, Error::Transport(_)));
        assert!(stream.has_pending_operation());
        assert!(!stream.is_finished());
        assert_eq!(stream.next_sequence(), 2);

        block_on(stream.finish()).expect("replayed finish");
        assert!(!stream.has_pending_operation());
        assert!(stream.is_finished());
        assert_eq!(stream.next_sequence(), 3);

        let calls = transport.calls();
        assert_eq!(calls[4].body["sequence"], 2);
        assert_eq!(calls[5].body["sequence"], 2);
        assert_eq!(calls[4].body["uuid"], calls[5].body["uuid"]);
        assert_eq!(calls[4].body["settings"], calls[5].body["settings"]);
    }

    #[test]
    fn http_status_during_finish_retains_the_original_operation() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157323"),
            message_response("om_http_finish"),
            ok_response(),
            HttpResponse::json(503, Value::Null),
            ok_response(),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);
        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .start(),
        )
        .expect("started stream");
        block_on(stream.set_content("done")).expect("content update");

        let error = block_on(stream.finish()).expect_err("ambiguous HTTP finish failure");
        assert!(matches!(error, Error::HttpStatus { status: 503 }));
        assert!(stream.has_pending_operation());

        block_on(stream.finish()).expect("replayed finish");
        assert!(stream.is_finished());
        let calls = transport.calls();
        assert_eq!(calls[4].body["sequence"], calls[5].body["sequence"]);
        assert_eq!(calls[4].body["uuid"], calls[5].body["uuid"]);
        assert_eq!(calls[4].body["settings"], calls[5].body["settings"]);
    }

    #[test]
    fn api_errors_do_not_retry_or_advance_local_state() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157316"),
            message_response("om_api_error"),
            HttpResponse::json(200, json!({ "code": 230099, "msg": "stale sequence" })),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);

        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .start(),
        )
        .expect("started stream");
        let error = block_on(stream.append("hello")).expect_err("API error");

        assert!(matches!(error, Error::Api { code: 230099, .. }));
        assert_eq!(transport.calls().len(), 4);
        assert_eq!(stream.content(), None);
        assert_eq!(stream.next_sequence(), 1);
        assert!(!stream.has_pending_operation());
    }

    #[test]
    fn http_status_errors_retain_pending_operations_for_explicit_replay() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157322"),
            message_response("om_http_error"),
            HttpResponse::json(503, Value::Null),
            ok_response(),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);

        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .start(),
        )
        .expect("started stream");
        let error = block_on(stream.append("hello")).expect_err("HTTP status error");

        assert!(matches!(error, Error::HttpStatus { status: 503 }));
        assert_eq!(transport.calls().len(), 4);
        assert_eq!(stream.content(), None);
        assert_eq!(stream.next_sequence(), 1);
        assert!(stream.has_pending_operation());

        block_on(stream.retry_pending()).expect("replayed content update");
        assert_eq!(stream.content(), Some("hello"));
        assert_eq!(stream.next_sequence(), 2);
        assert!(!stream.has_pending_operation());

        let calls = transport.calls();
        assert_eq!(calls[3].body["sequence"], calls[4].body["sequence"]);
        assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
        assert_eq!(calls[3].body["content"], calls[4].body["content"]);
    }

    #[test]
    fn oversized_updates_fail_without_advancing_sequence_or_content() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157317"),
            message_response("om_oversized"),
        ]);
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        let sender = MessageSender::new(client);

        let mut stream = block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .start(),
        )
        .expect("started stream");
        let error = block_on(stream.set_content("x".repeat(31_000)))
            .expect_err("whole card exceeds 30 KiB");

        assert!(matches!(error, Error::Validation(_)));
        assert_eq!(transport.calls().len(), 3);
        assert_eq!(stream.content(), None);
        assert_eq!(stream.next_sequence(), 1);
    }
}
