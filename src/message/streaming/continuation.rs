use std::collections::VecDeque;
use std::time::Duration;

use crate::lark_openapi::OpenApiTransport;
use crate::message::sender::MessageSender;
use crate::{CardId, Error, MessageId, Result};

use super::builder::{MarkdownStreamBuilder, MarkdownStreamTemplate};
use super::{MarkdownStream, ThrottledMarkdownStream};

mod split;

pub(super) use split::{DEFAULT_CONTINUATION_MAX_PAGE_CHARS, validate_continuation_limit};
use split::{PlannedPage, plan_content};

/// One CardKit entity and message created by a continuing Markdown stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownStreamPage {
    card_id: CardId,
    message_id: MessageId,
}

impl MarkdownStreamPage {
    fn new(card_id: CardId, message_id: MessageId) -> Self {
        Self {
            card_id,
            message_id,
        }
    }

    /// Returns the CardKit entity for this page.
    pub fn card_id(&self) -> &CardId {
        &self.card_id
    }

    /// Returns the message displaying this page.
    pub fn message_id(&self) -> &MessageId {
        &self.message_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RolloverStage {
    UpdateCurrent,
    FinishCurrent,
    StartNext,
}

#[derive(Debug)]
struct PendingRollover<'a, T> {
    current_page: PlannedPage,
    remaining_pages: VecDeque<PlannedPage>,
    next_builder: MarkdownStreamBuilder<'a, T>,
    stage: RolloverStage,
}

/// A Markdown stream that rolls long output onto additional CardKit messages.
///
/// The controller keeps accepted source content and already closed pages
/// immutable. `set_content` therefore accepts monotonically extending complete
/// snapshots. Pagination is format-agnostic: it preserves source text and UTF-8
/// character boundaries but does not rewrite Markdown or guarantee that a
/// construct stays on one page. Ambiguous remote failures retain the exact
/// in-progress update, finish, or follow-up send for [`Self::retry_pending`].
#[derive(Debug)]
pub struct ContinuingMarkdownStream<'a, T> {
    sender: &'a MessageSender<T>,
    template: MarkdownStreamTemplate,
    active: ThrottledMarkdownStream<'a, T>,
    pages: Vec<MarkdownStreamPage>,
    source_content: String,
    finalized_source_len: usize,
    max_page_chars: usize,
    rollover: Option<PendingRollover<'a, T>>,
    finishing: bool,
}

impl<'a, T> ContinuingMarkdownStream<'a, T>
where
    T: OpenApiTransport,
{
    pub(super) fn new(
        sender: &'a MessageSender<T>,
        stream: MarkdownStream<'a, T>,
        template: MarkdownStreamTemplate,
        max_page_chars: usize,
    ) -> Self {
        let first_page =
            MarkdownStreamPage::new(stream.card_id().clone(), stream.message_id().clone());
        Self {
            sender,
            template,
            active: stream.throttle(Duration::ZERO),
            pages: vec![first_page],
            source_content: String::new(),
            finalized_source_len: 0,
            max_page_chars,
            rollover: None,
            finishing: false,
        }
    }

    /// Applies one coalescing interval to every current and future page.
    pub fn throttle(mut self, min_update_interval: Duration) -> Self {
        self.active.set_min_update_interval(min_update_interval);
        self
    }

    /// Returns the complete original Markdown source accepted by the stream.
    pub fn content(&self) -> Option<&str> {
        (!self.source_content.is_empty()).then_some(self.source_content.as_str())
    }

    /// Returns all pages in creation order.
    pub fn pages(&self) -> &[MarkdownStreamPage] {
        &self.pages
    }

    /// Returns the first message in the continuation chain.
    pub fn first_message_id(&self) -> &MessageId {
        self.pages
            .first()
            .expect("a continuing stream always has one page")
            .message_id()
    }

    /// Returns the message currently receiving content.
    pub fn current_message_id(&self) -> &MessageId {
        self.active.message_id()
    }

    /// Returns the CardKit entity currently receiving content.
    pub fn current_card_id(&self) -> &CardId {
        self.active.card_id()
    }

    /// Returns whether the last page has been closed successfully.
    pub fn is_finished(&self) -> bool {
        self.rollover.is_none() && self.active.is_finished()
    }

    /// Returns whether a failed multi-step rollover or CardKit operation is
    /// retained for replay.
    pub fn has_pending_operation(&self) -> bool {
        !self.is_recovery_blocked()
            && (self.finishing || self.rollover.is_some() || self.active.has_pending_operation())
    }

    /// Returns whether an ambiguous non-idempotent card creation prevents
    /// further automatic recovery.
    pub fn is_recovery_blocked(&self) -> bool {
        self.rollover.as_ref().is_some_and(|pending| {
            pending.stage == RolloverStage::StartNext
                && pending.next_builder.has_unknown_card_creation_outcome()
        })
    }

    /// Returns how long remains before buffered current-page content is due.
    pub fn next_flush_in(&self) -> Option<Duration> {
        if self.is_recovery_blocked() {
            None
        } else if self.finishing || self.rollover.is_some() {
            Some(Duration::ZERO)
        } else {
            self.active.next_flush_in()
        }
    }

    /// Appends one source delta and rolls over as many cards as necessary.
    pub async fn append(&mut self, chunk: impl AsRef<str>) -> Result<()> {
        self.ensure_mutable()?;
        let chunk = chunk.as_ref();
        if chunk.is_empty() {
            return Ok(());
        }

        let projected_finalized_source_len = self.projected_finalized_source_len();
        let active_source = &self.source_content[projected_finalized_source_len..];
        let minimum_first_page_source_bytes = active_source.len();
        let mut next_active_source = String::with_capacity(active_source.len() + chunk.len());
        next_active_source.push_str(active_source);
        next_active_source.push_str(chunk);
        let plan = plan_content(
            &next_active_source,
            self.template.profile(),
            self.max_page_chars,
            minimum_first_page_source_bytes,
        )?;

        self.recover_pending().await?;
        debug_assert_eq!(self.finalized_source_len, projected_finalized_source_len);
        self.source_content.push_str(chunk);
        self.apply_content_plan(plan).await
    }

    /// Replaces the complete source snapshot with a monotonic extension.
    ///
    /// Once output has been accepted, later snapshots must start with all prior
    /// source content. This prevents a rollover from removing content already
    /// displayed or buffered on the active page.
    pub async fn set_content(&mut self, content: impl Into<String>) -> Result<()> {
        self.ensure_mutable()?;
        let content = content.into();
        self.validate_replacement_source(&content)?;
        let projected_finalized_source_len = self.projected_finalized_source_len();
        let current = &content[projected_finalized_source_len..];
        let minimum_first_page_source_bytes = self
            .source_content
            .len()
            .saturating_sub(projected_finalized_source_len);
        let plan = if current.is_empty() {
            None
        } else {
            Some(plan_content(
                current,
                self.template.profile(),
                self.max_page_chars,
                minimum_first_page_source_bytes,
            )?)
        };

        self.recover_pending().await?;
        debug_assert_eq!(self.finalized_source_len, projected_finalized_source_len);
        self.source_content = content;
        match plan {
            Some(plan) => self.apply_content_plan(plan).await,
            None => Ok(()),
        }
    }

    /// Retries retained work using its original sequence, UUID, and prepared
    /// follow-up card when available.
    ///
    /// Card entity creation has no idempotency key. If a creation outcome is
    /// unknown, retry returns a validation error instead of creating another
    /// entity; the caller must stop or reconcile that terminal ambiguity.
    pub async fn retry_pending(&mut self) -> Result<()> {
        if self.finishing {
            self.resume_finish().await
        } else {
            self.recover_pending().await
        }
    }

    /// Flushes one due current-page update or resumes a pending rollover.
    pub async fn flush_if_due(&mut self) -> Result<bool> {
        if self.finishing {
            self.resume_finish().await?;
            return Ok(true);
        }
        if self.rollover.is_some() {
            self.resume_rollover().await?;
            return Ok(true);
        }
        self.active.flush_if_due().await
    }

    /// Immediately completes pending rollover work and flushes the current page.
    pub async fn flush(&mut self) -> Result<()> {
        if self.is_finished() {
            return Ok(());
        }
        if self.finishing {
            return self.resume_finish().await;
        }
        self.recover_pending().await?;
        self.active.flush().await
    }

    /// Flushes all remaining content and closes the last card.
    pub async fn finish(&mut self) -> Result<()> {
        if self.is_finished() {
            return Ok(());
        }
        self.finishing = true;
        self.resume_finish().await
    }

    fn validate_replacement_source(&self, content: &str) -> Result<()> {
        if !content.starts_with(&self.source_content) {
            return Err(Error::Validation(
                "markdown continuation snapshots must preserve all previously accepted content"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn projected_finalized_source_len(&self) -> usize {
        let mut finalized_source_len = self.finalized_source_len;
        let Some(pending) = &self.rollover else {
            return finalized_source_len;
        };

        for page in std::iter::once(&pending.current_page)
            .chain(pending.remaining_pages.iter())
            .take(pending.remaining_pages.len())
        {
            finalized_source_len += page.content.len();
        }
        finalized_source_len
    }

    async fn recover_pending(&mut self) -> Result<()> {
        if self.rollover.is_some() {
            self.resume_rollover().await?;
        } else if self.active.has_pending_operation() {
            self.active.flush().await?;
        }
        Ok(())
    }

    async fn resume_finish(&mut self) -> Result<()> {
        self.recover_pending().await?;
        self.active.finish().await?;
        self.finishing = false;
        Ok(())
    }

    async fn apply_content_plan(&mut self, mut pages: VecDeque<PlannedPage>) -> Result<()> {
        let current_page = pages
            .pop_front()
            .expect("nonempty source always produces a page plan");
        if pages.is_empty() {
            return self.active.set_content(current_page.content).await;
        }

        self.rollover = Some(PendingRollover {
            current_page,
            remaining_pages: pages,
            next_builder: self.template.follow_up_builder(self.sender),
            stage: RolloverStage::UpdateCurrent,
        });
        self.resume_rollover().await
    }

    async fn resume_rollover(&mut self) -> Result<()> {
        loop {
            let Some(stage) = self.rollover.as_ref().map(|pending| pending.stage) else {
                return Ok(());
            };

            let result = match stage {
                RolloverStage::UpdateCurrent => {
                    let content = self
                        .rollover
                        .as_ref()
                        .expect("rollover exists for its current stage")
                        .current_page
                        .content
                        .clone();
                    if let Err(error) = self.active.set_content(content).await {
                        Err(error)
                    } else {
                        self.active.flush().await
                    }
                }
                RolloverStage::FinishCurrent => self.active.finish().await,
                RolloverStage::StartNext => match self
                    .rollover
                    .as_mut()
                    .expect("rollover exists for its current stage")
                    .next_builder
                    .start()
                    .await
                {
                    Ok(stream) => {
                        let pending = self
                            .rollover
                            .take()
                            .expect("successful follow-up start completes the rollover");
                        let interval = self.active.min_update_interval();
                        let page = MarkdownStreamPage::new(
                            stream.card_id().clone(),
                            stream.message_id().clone(),
                        );
                        self.active = stream.throttle(interval);
                        self.finalized_source_len += pending.current_page.content.len();
                        self.pages.push(page);

                        let mut remaining_pages = pending.remaining_pages;
                        let current_page = remaining_pages
                            .pop_front()
                            .expect("a rollover always retains a follow-up page");
                        if remaining_pages.is_empty() {
                            return self.active.set_content(current_page.content).await;
                        }

                        self.rollover = Some(PendingRollover {
                            current_page,
                            remaining_pages,
                            next_builder: self.template.follow_up_builder(self.sender),
                            stage: RolloverStage::UpdateCurrent,
                        });
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
            };

            result?;

            if stage == RolloverStage::StartNext {
                continue;
            }

            self.rollover
                .as_mut()
                .expect("successful nonterminal stage keeps the rollover")
                .stage = match stage {
                RolloverStage::UpdateCurrent => RolloverStage::FinishCurrent,
                RolloverStage::FinishCurrent => RolloverStage::StartNext,
                RolloverStage::StartNext => unreachable!("successful start returns above"),
            };
        }
    }

    fn ensure_mutable(&self) -> Result<()> {
        if self.finishing
            || (self.rollover.is_none()
                && (self.active.is_finished() || self.active.has_pending_finish()))
        {
            return Err(Error::Validation(
                "markdown continuation stream is already finished or finishing".to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
