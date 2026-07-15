use std::time::{Duration, Instant};

use crate::lark_openapi::OpenApiTransport;
use crate::{CardId, Error, MessageId, Result};

use super::MarkdownStream;

/// Runtime-independent coalescing throttle for a high-level Markdown stream.
///
/// The first generated content is sent immediately. Later `append` and
/// `set_content` calls retain the latest complete content locally and send it
/// only after `min_update_interval` has elapsed. No background timer is
/// created: a later content call, `flush`, or `finish` drives delivery.
/// Applications that need buffered content to appear during a producer pause
/// can schedule `flush` using [`Self::next_flush_in`].
///
/// The current platform guidance limits card operations on one entity to ten
/// per second. Callers that want the wrapper to enforce that conservative rate
/// should use an interval of at least 100 milliseconds. The interval itself is
/// not restricted so tests and applications with stricter external scheduling
/// can choose their own policy.
#[derive(Debug)]
pub struct ThrottledMarkdownStream<'a, T> {
    stream: MarkdownStream<'a, T>,
    min_update_interval: Duration,
    desired_content: Option<String>,
    last_update_attempt_at: Option<Instant>,
}

impl<'a, T> ThrottledMarkdownStream<'a, T>
where
    T: OpenApiTransport,
{
    pub(super) fn new(stream: MarkdownStream<'a, T>, min_update_interval: Duration) -> Self {
        let desired_content = stream
            .pending_content()
            .or_else(|| stream.content())
            .map(str::to_owned);
        let last_update_attempt_at =
            (stream.content().is_some() || stream.has_pending_operation()).then(Instant::now);
        Self {
            stream,
            min_update_interval,
            desired_content,
            last_update_attempt_at,
        }
    }

    /// Returns the message that displays this stream.
    pub fn message_id(&self) -> &MessageId {
        self.stream.message_id()
    }

    /// Returns the CardKit entity updated by this stream.
    pub fn card_id(&self) -> &CardId {
        self.stream.card_id()
    }

    /// Returns the complete locally accumulated content, including data that
    /// may still be buffered.
    pub fn content(&self) -> Option<&str> {
        self.desired_content.as_deref()
    }

    /// Returns the latest content acknowledged by the OpenAPI client.
    pub fn flushed_content(&self) -> Option<&str> {
        self.stream.content()
    }

    /// Returns the configured minimum interval between automatic update
    /// attempts.
    pub fn min_update_interval(&self) -> Duration {
        self.min_update_interval
    }

    /// Returns the sequence that will be used by the next CardKit operation.
    pub fn next_sequence(&self) -> u32 {
        self.stream.next_sequence()
    }

    /// Returns whether the stream completed successfully.
    pub fn is_finished(&self) -> bool {
        self.stream.is_finished()
    }

    /// Returns whether a transport failure retained an operation for replay.
    pub fn has_pending_operation(&self) -> bool {
        self.stream.has_pending_operation()
    }

    /// Returns whether locally accumulated content has not been acknowledged
    /// by the OpenAPI client yet.
    pub fn has_buffered_content(&self) -> bool {
        self.desired_content.as_deref() != self.stream.content()
    }

    /// Returns how long the caller should wait before an automatic flush is
    /// due, or `None` when there is no buffered or pending content operation.
    pub fn next_flush_in(&self) -> Option<Duration> {
        if self.stream.has_pending_finish()
            || (!self.has_buffered_content() && !self.stream.has_pending_operation())
        {
            return None;
        }

        Some(match self.last_update_attempt_at {
            Some(last_attempt) => self
                .min_update_interval
                .saturating_sub(last_attempt.elapsed()),
            None => Duration::ZERO,
        })
    }

    /// Appends one token delta to the local content and flushes it when the
    /// throttle window is due.
    ///
    /// Empty chunks are a no-op. Validation runs before the local buffer is
    /// changed, so rejected content does not poison a later flush.
    pub async fn append(&mut self, chunk: impl AsRef<str>) -> Result<()> {
        self.ensure_content_mutable()?;
        let chunk = chunk.as_ref();
        if chunk.is_empty() {
            return Ok(());
        }

        let mut content = self.content().unwrap_or_default().to_owned();
        content.push_str(chunk);
        self.stream.validate_content(&content)?;
        self.desired_content = Some(content);
        self.flush_if_due().await.map(|_| ())
    }

    /// Replaces the local complete content snapshot and flushes it when the
    /// throttle window is due.
    pub async fn set_content(&mut self, content: impl Into<String>) -> Result<()> {
        self.ensure_content_mutable()?;
        let content = content.into();
        self.stream.validate_content(&content)?;
        self.desired_content = Some(content);
        self.flush_if_due().await.map(|_| ())
    }

    /// Flushes one retained operation or buffered content update when the
    /// throttle interval has elapsed.
    ///
    /// Returns `true` when an OpenAPI operation was acknowledged and `false`
    /// when there was no due work. A retained ambiguous operation is replayed
    /// before newer buffered content and keeps its original sequence and UUID.
    pub async fn flush_if_due(&mut self) -> Result<bool> {
        if self.stream.is_finished() || self.stream.has_pending_finish() {
            return Ok(false);
        }
        let now = Instant::now();
        if !self.is_due_at(now) {
            return Ok(false);
        }

        if self.stream.has_pending_operation() {
            self.last_update_attempt_at = Some(now);
            self.stream.retry_pending().await?;
            return Ok(true);
        }

        self.flush_buffered_at(now).await
    }

    /// Immediately replays retained work and sends the latest buffered
    /// content, bypassing the throttle interval.
    pub async fn flush(&mut self) -> Result<()> {
        if self.stream.is_finished() {
            return Ok(());
        }
        if self.stream.has_pending_operation() {
            self.last_update_attempt_at = Some(Instant::now());
            self.stream.retry_pending().await?;
            if self.stream.is_finished() {
                self.sync_content_from_stream();
                return Ok(());
            }
        }
        self.flush_buffered_at(Instant::now()).await.map(|_| ())
    }

    /// Flushes the latest content and disables CardKit streaming mode.
    ///
    /// Repeating `finish` after an ambiguous finish failure replays the
    /// retained settings operation through the underlying stream.
    pub async fn finish(&mut self) -> Result<()> {
        if self.stream.is_finished() {
            self.sync_content_from_stream();
            return Ok(());
        }
        if self.stream.has_pending_finish() {
            let result = self.stream.retry_pending().await;
            self.sync_content_from_stream();
            return result;
        }

        self.flush().await?;
        if self.stream.is_finished() {
            return Ok(());
        }
        let result = self.stream.finish().await;
        self.sync_content_from_stream();
        result
    }

    fn is_due_at(&self, now: Instant) -> bool {
        if !self.has_buffered_content() && !self.stream.has_pending_operation() {
            return false;
        }
        self.last_update_attempt_at.is_none_or(|last_attempt| {
            now.saturating_duration_since(last_attempt) >= self.min_update_interval
        })
    }

    async fn flush_buffered_at(&mut self, now: Instant) -> Result<bool> {
        if !self.has_buffered_content() {
            return Ok(false);
        }
        let Some(content) = self.desired_content.clone() else {
            return Ok(false);
        };

        self.last_update_attempt_at = Some(now);
        self.stream.set_content(content).await?;
        Ok(true)
    }

    fn ensure_content_mutable(&self) -> Result<()> {
        if self.stream.is_finished() {
            return Err(Error::Validation(
                "markdown stream is already finished".to_owned(),
            ));
        }
        if self.stream.has_pending_finish() {
            return Err(Error::Validation(
                "retry the pending markdown stream finish before changing content".to_owned(),
            ));
        }
        Ok(())
    }

    fn sync_content_from_stream(&mut self) {
        self.desired_content = self
            .stream
            .pending_content()
            .or_else(|| self.stream.content())
            .map(str::to_owned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lark_openapi::HttpMethod;
    use crate::message::streaming::test_support::{
        FakeTransport, assert_card_content_update, block_on, card_response, message_response,
        ok_response, token_response,
    };
    use crate::{ChannelConfig, MessageSender, Recipient};

    const INTERVAL: Duration = Duration::from_millis(100);

    #[test]
    fn coalesces_updates_and_flushes_the_tail_before_finish() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157401"),
            message_response("om_throttled"),
            ok_response(),
            ok_response(),
            ok_response(),
            ok_response(),
        ]);
        let sender = sender(&transport);
        let mut stream = started_stream(&sender).throttle(INTERVAL);

        block_on(stream.append("A")).expect("first update");
        block_on(stream.append("B")).expect("buffered second update");
        assert_eq!(transport.calls().len(), 4);
        assert_eq!(stream.content(), Some("AB"));
        assert_eq!(stream.flushed_content(), Some("A"));
        assert!(stream.has_buffered_content());
        assert!(stream.next_flush_in().is_some_and(|wait| wait <= INTERVAL));

        make_due(&mut stream);
        block_on(stream.append("C")).expect("coalesced update");
        block_on(stream.append("D")).expect("buffered tail");
        block_on(stream.finish()).expect("flushed and finished");

        assert_eq!(stream.content(), Some("ABCD"));
        assert_eq!(stream.flushed_content(), Some("ABCD"));
        assert!(!stream.has_buffered_content());
        assert!(stream.is_finished());
        assert_eq!(stream.next_sequence(), 5);
        assert_eq!(stream.next_flush_in(), None);

        let calls = transport.calls();
        assert_eq!(calls.len(), 7);
        assert_card_content_update(&calls[3], 1, "A");
        assert_card_content_update(&calls[4], 2, "ABC");
        assert_card_content_update(&calls[5], 3, "ABCD");
        assert_eq!(calls[6].method, HttpMethod::Patch);
        assert_eq!(calls[6].body["sequence"], 4);
    }

    #[test]
    fn explicit_flush_bypasses_the_interval_without_duplicate_updates() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157402"),
            message_response("om_flush"),
            ok_response(),
            ok_response(),
        ]);
        let sender = sender(&transport);
        let mut stream = started_stream(&sender).throttle(INTERVAL);

        block_on(stream.append("A")).expect("first update");
        block_on(stream.append("B")).expect("buffered update");
        block_on(stream.flush()).expect("explicit flush");
        let calls_after_flush = transport.calls().len();
        block_on(stream.flush()).expect("duplicate flush is a no-op");

        assert_eq!(calls_after_flush, 5);
        assert_eq!(transport.calls().len(), calls_after_flush);
        assert_eq!(stream.flushed_content(), Some("AB"));
    }

    #[test]
    fn replays_pending_content_before_flushing_newer_buffered_content() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157403")),
            Ok(message_response("om_pending")),
            Err(Error::Transport("content response lost".to_owned())),
            Ok(ok_response()),
            Ok(ok_response()),
        ]);
        let sender = sender(&transport);
        let mut stream = started_stream(&sender).throttle(INTERVAL);

        let error = block_on(stream.append("A")).expect_err("ambiguous first update");
        assert!(matches!(error, Error::Transport(_)));
        assert!(stream.has_pending_operation());
        assert_eq!(stream.content(), Some("A"));
        assert_eq!(stream.flushed_content(), None);

        block_on(stream.append("B")).expect("new content stays buffered");
        assert_eq!(transport.calls().len(), 4);
        make_due(&mut stream);
        assert!(block_on(stream.flush_if_due()).expect("pending replay"));
        assert_eq!(stream.flushed_content(), Some("A"));
        assert_eq!(stream.content(), Some("AB"));
        assert!(stream.has_buffered_content());

        block_on(stream.flush()).expect("newer buffer flushed");
        assert_eq!(stream.flushed_content(), Some("AB"));
        assert!(!stream.has_pending_operation());

        let calls = transport.calls();
        assert_eq!(calls.len(), 6);
        assert_eq!(calls[3].body["sequence"], 1);
        assert_eq!(calls[4].body["sequence"], 1);
        assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
        assert_card_content_update(&calls[5], 2, "AB");
    }

    #[test]
    fn invalid_buffered_content_does_not_replace_the_previous_snapshot() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157404"),
            message_response("om_validation"),
            ok_response(),
        ]);
        let sender = sender(&transport);
        let mut stream = started_stream(&sender).throttle(INTERVAL);

        block_on(stream.append("valid")).expect("first update");
        let error = block_on(stream.set_content(String::new())).expect_err("empty content");
        assert!(matches!(error, Error::Validation(_)));
        assert_eq!(stream.content(), Some("valid"));
        assert_eq!(stream.flushed_content(), Some("valid"));
        assert!(!stream.has_buffered_content());
        assert_eq!(transport.calls().len(), 4);
    }

    #[test]
    fn empty_finish_synchronizes_the_fallback_content() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157405"),
            message_response("om_empty"),
            ok_response(),
            ok_response(),
        ]);
        let sender = sender(&transport);
        let mut stream = started_stream(&sender).throttle(INTERVAL);

        block_on(stream.finish()).expect("empty stream finished");

        assert_eq!(stream.content(), Some("(no content)"));
        assert_eq!(stream.flushed_content(), Some("(no content)"));
        assert!(!stream.has_buffered_content());
        assert!(stream.is_finished());
    }

    #[test]
    fn wrapping_an_updated_stream_starts_a_new_throttle_window() {
        let transport = FakeTransport::http(vec![
            token_response(),
            card_response("7355372766134157406"),
            message_response("om_existing"),
            ok_response(),
        ]);
        let sender = sender(&transport);
        let mut immediate = started_stream(&sender);
        block_on(immediate.append("A")).expect("immediate update");

        let mut stream = immediate.throttle(INTERVAL);
        block_on(stream.append("B")).expect("buffered after wrapping");

        assert_eq!(transport.calls().len(), 4);
        assert_eq!(stream.content(), Some("AB"));
        assert_eq!(stream.flushed_content(), Some("A"));
        assert!(stream.has_buffered_content());
    }

    #[test]
    fn wrapping_a_pending_stream_preserves_the_attempted_content() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157407")),
            Ok(message_response("om_existing_pending")),
            Err(Error::Transport("content response lost".to_owned())),
            Ok(ok_response()),
        ]);
        let sender = sender(&transport);
        let mut immediate = started_stream(&sender);
        let error = block_on(immediate.append("A")).expect_err("ambiguous update");
        assert!(matches!(error, Error::Transport(_)));

        let mut stream = immediate.throttle(INTERVAL);
        assert_eq!(stream.content(), Some("A"));
        assert_eq!(stream.flushed_content(), None);
        assert!(stream.has_pending_operation());
        make_due(&mut stream);
        assert!(block_on(stream.flush_if_due()).expect("pending replay"));

        assert_eq!(stream.content(), Some("A"));
        assert_eq!(stream.flushed_content(), Some("A"));
        assert!(!stream.has_buffered_content());
        assert!(!stream.has_pending_operation());
    }

    #[test]
    fn pending_empty_finish_keeps_fallback_content_and_replays_settings() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157408")),
            Ok(message_response("om_pending_finish")),
            Ok(ok_response()),
            Err(Error::Transport("finish response lost".to_owned())),
            Ok(ok_response()),
        ]);
        let sender = sender(&transport);
        let mut stream = started_stream(&sender).throttle(INTERVAL);

        let error = block_on(stream.finish()).expect_err("ambiguous finish");
        assert!(matches!(error, Error::Transport(_)));
        assert_eq!(stream.content(), Some("(no content)"));
        assert_eq!(stream.flushed_content(), Some("(no content)"));
        assert!(!stream.has_buffered_content());
        assert!(stream.has_pending_operation());

        let calls_before_append = transport.calls().len();
        let error = block_on(stream.append("late")).expect_err("finish remains pending");
        assert!(matches!(error, Error::Validation(_)));
        assert_eq!(transport.calls().len(), calls_before_append);

        block_on(stream.finish()).expect("finish replayed");
        assert!(stream.is_finished());
        let calls = transport.calls();
        assert_eq!(calls[4].body["sequence"], 2);
        assert_eq!(calls[5].body["sequence"], 2);
        assert_eq!(calls[4].body["uuid"], calls[5].body["uuid"]);
    }

    #[test]
    fn pending_empty_fallback_is_preserved_before_finish_can_start() {
        let transport = FakeTransport::new(vec![
            Ok(token_response()),
            Ok(card_response("7355372766134157409")),
            Ok(message_response("om_pending_fallback")),
            Err(Error::Transport("fallback response lost".to_owned())),
            Ok(ok_response()),
            Ok(ok_response()),
        ]);
        let sender = sender(&transport);
        let mut stream = started_stream(&sender).throttle(INTERVAL);

        let error = block_on(stream.finish()).expect_err("ambiguous fallback update");
        assert!(matches!(error, Error::Transport(_)));
        assert_eq!(stream.content(), Some("(no content)"));
        assert_eq!(stream.flushed_content(), None);
        assert!(stream.has_buffered_content());
        assert!(stream.has_pending_operation());

        make_due(&mut stream);
        assert!(block_on(stream.flush_if_due()).expect("fallback replay"));
        assert_eq!(stream.content(), Some("(no content)"));
        assert_eq!(stream.flushed_content(), Some("(no content)"));
        assert!(!stream.has_buffered_content());
        assert!(!stream.has_pending_operation());

        block_on(stream.finish()).expect("settings update");
        assert!(stream.is_finished());
        let calls = transport.calls();
        assert_eq!(calls[3].body["sequence"], 1);
        assert_eq!(calls[4].body["sequence"], 1);
        assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
        assert_eq!(calls[5].body["sequence"], 2);
    }

    fn sender(transport: &FakeTransport) -> MessageSender<FakeTransport> {
        let client = crate::lark_openapi::OpenApiClient::new(
            ChannelConfig::new("cli_a", "secret"),
            transport.clone(),
        );
        MessageSender::new(client)
    }

    fn started_stream<'a>(
        sender: &'a MessageSender<FakeTransport>,
    ) -> MarkdownStream<'a, FakeTransport> {
        block_on(
            sender
                .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
                .max_attempts(1)
                .start(),
        )
        .expect("started stream")
    }

    fn make_due<T>(stream: &mut ThrottledMarkdownStream<'_, T>) {
        stream.last_update_attempt_at = Some(
            Instant::now()
                .checked_sub(stream.min_update_interval)
                .expect("past instant"),
        );
    }
}
