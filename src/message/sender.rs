//! High-level outbound message sender.
//!
//! `MessageSender` wraps the low-level `lark_openapi::OpenApiClient` with
//! Channel-oriented behavior such as automatic idempotency keys and conservative
//! retry handling.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::lark_openapi::{
    MessageCreateOptions, MessageReplyOptions, OpenApiClient, OpenApiTransport,
};
use crate::{Error, MessageContent, MessageId, Recipient, Result};

const MAX_OPENAPI_UUID_CHARS: usize = 50;
static NEXT_IDEMPOTENCY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// High-level sender for outbound text messages and replies.
///
/// `MessageSender` generates an idempotency key once per logical send/reply by
/// default, or accepts a caller-provided `uuid`, and reuses that value across
/// transport-failure retries. API and OpenAPI HTTP status errors are returned
/// directly.
#[derive(Debug, Clone)]
pub struct MessageSender<T> {
    client: OpenApiClient<T>,
    options: MessageSenderOptions,
}

impl<T> MessageSender<T>
where
    T: OpenApiTransport,
{
    /// Creates a sender with default options.
    pub fn new(client: OpenApiClient<T>) -> Self {
        Self {
            client,
            options: MessageSenderOptions::default(),
        }
    }

    /// Creates a sender with caller-provided options.
    pub fn with_options(client: OpenApiClient<T>, options: MessageSenderOptions) -> Self {
        Self { client, options }
    }

    /// Returns the wrapped low-level OpenAPI client.
    pub fn client(&self) -> &OpenApiClient<T> {
        &self.client
    }

    /// Returns the sender options.
    pub fn options(&self) -> &MessageSenderOptions {
        &self.options
    }

    /// Starts building a message send operation with caller-provided content.
    pub fn message(&self, recipient: Recipient, content: MessageContent) -> MessageBuilder<'_, T> {
        MessageBuilder {
            sender: self,
            recipient,
            content,
            uuid: None,
            max_attempts: None,
        }
    }

    /// Starts building a plain text message send operation.
    pub fn text_message(
        &self,
        recipient: Recipient,
        text: impl Into<String>,
    ) -> MessageBuilder<'_, T> {
        self.message(recipient, MessageContent::Text { text: text.into() })
    }

    /// Starts building a message reply operation with caller-provided content.
    pub fn reply(
        &self,
        parent_message_id: MessageId,
        content: MessageContent,
    ) -> MessageReplyBuilder<'_, T> {
        MessageReplyBuilder {
            sender: self,
            parent_message_id,
            content,
            uuid: None,
            reply_in_thread: None,
            max_attempts: None,
        }
    }

    /// Starts building a plain text reply operation.
    pub fn text_reply(
        &self,
        parent_message_id: MessageId,
        text: impl Into<String>,
    ) -> MessageReplyBuilder<'_, T> {
        self.reply(
            parent_message_id,
            MessageContent::Text { text: text.into() },
        )
    }

    async fn retry_transport_errors<F, Fut>(
        &self,
        max_attempts: usize,
        mut operation: F,
    ) -> Result<MessageId>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<MessageId>>,
    {
        let mut attempts = 0;

        loop {
            attempts += 1;
            match operation().await {
                Ok(message_id) => return Ok(message_id),
                Err(error) if attempts < max_attempts && is_retryable(&error) => {}
                Err(error) => return Err(error),
            }
        }
    }

    fn max_attempts(&self, max_attempts: Option<usize>) -> usize {
        max_attempts
            .unwrap_or_else(|| self.options.max_attempts())
            .max(1)
    }
}

/// Builder for a single message send operation.
#[derive(Debug)]
pub struct MessageBuilder<'a, T> {
    sender: &'a MessageSender<T>,
    recipient: Recipient,
    content: MessageContent,
    uuid: Option<String>,
    max_attempts: Option<usize>,
}

impl<T> MessageBuilder<'_, T>
where
    T: OpenApiTransport,
{
    /// Sets the caller-provided OpenAPI `uuid`.
    ///
    /// Use this when an upstream system already has a stable request, task, or
    /// event identifier that should be reused across process restarts or queue
    /// replays.
    pub fn uuid(mut self, uuid: impl Into<String>) -> Self {
        self.uuid = Some(uuid.into());
        self
    }

    /// Sets the maximum number of attempts for this send operation.
    ///
    /// Values below `1` are clamped to `1`.
    pub fn max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = Some(max_attempts.max(1));
        self
    }

    /// Sends the message.
    pub async fn send(self) -> Result<MessageId> {
        let uuid = resolve_uuid(self.uuid)?;
        let max_attempts = self.sender.max_attempts(self.max_attempts);
        let recipient = self.recipient;
        let content = self.content;

        self.sender
            .retry_transport_errors(max_attempts, || {
                self.sender.client.create_message_with_options(
                    recipient.clone(),
                    content.clone(),
                    MessageCreateOptions::with_uuid(uuid.clone()),
                )
            })
            .await
    }
}

/// Builder for a single message reply operation.
#[derive(Debug)]
pub struct MessageReplyBuilder<'a, T> {
    sender: &'a MessageSender<T>,
    parent_message_id: MessageId,
    content: MessageContent,
    uuid: Option<String>,
    reply_in_thread: Option<bool>,
    max_attempts: Option<usize>,
}

impl<T> MessageReplyBuilder<'_, T>
where
    T: OpenApiTransport,
{
    /// Sets the caller-provided OpenAPI `uuid`.
    ///
    /// Use this when an upstream system already has a stable request, task, or
    /// event identifier that should be reused across process restarts or queue
    /// replays.
    pub fn uuid(mut self, uuid: impl Into<String>) -> Self {
        self.uuid = Some(uuid.into());
        self
    }

    /// Requests thread placement when the target conversation supports it.
    pub fn reply_in_thread(mut self, reply_in_thread: bool) -> Self {
        self.reply_in_thread = Some(reply_in_thread);
        self
    }

    /// Sets the maximum number of attempts for this reply operation.
    ///
    /// Values below `1` are clamped to `1`.
    pub fn max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = Some(max_attempts.max(1));
        self
    }

    /// Sends the reply.
    pub async fn send(self) -> Result<MessageId> {
        let uuid = resolve_uuid(self.uuid)?;
        let max_attempts = self.sender.max_attempts(self.max_attempts);
        let parent_message_id = self.parent_message_id;
        let content = self.content;
        let reply_in_thread = self.reply_in_thread;

        self.sender
            .retry_transport_errors(max_attempts, || {
                let mut reply_options = MessageReplyOptions::with_uuid(uuid.clone());
                if let Some(reply_in_thread) = reply_in_thread {
                    reply_options = reply_options.reply_in_thread(reply_in_thread);
                }
                self.sender.client.reply_message_with_options(
                    parent_message_id.clone(),
                    content.clone(),
                    reply_options,
                )
            })
            .await
    }
}

/// Options for `MessageSender`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSenderOptions {
    max_attempts: usize,
}

impl Default for MessageSenderOptions {
    fn default() -> Self {
        Self { max_attempts: 3 }
    }
}

impl MessageSenderOptions {
    /// Creates options with the default retry attempt count.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates options with a custom maximum attempt count.
    ///
    /// Values below `1` are clamped to `1`.
    pub fn with_max_attempts(max_attempts: usize) -> Self {
        let mut options = Self::new();
        options.set_max_attempts(max_attempts);
        options
    }

    /// Sets the maximum number of attempts for one logical send or reply.
    ///
    /// Values below `1` are clamped to `1`.
    pub fn set_max_attempts(&mut self, max_attempts: usize) -> &mut Self {
        self.max_attempts = max_attempts.max(1);
        self
    }

    /// Returns the configured maximum attempt count.
    pub fn max_attempts(&self) -> usize {
        self.max_attempts.max(1)
    }
}

fn is_retryable(error: &Error) -> bool {
    matches!(error, Error::Transport(_))
}

fn resolve_uuid(uuid: Option<String>) -> Result<String> {
    match uuid {
        Some(uuid) => validate_uuid(uuid),
        None => Ok(generate_idempotency_key()),
    }
}

fn validate_uuid(uuid: String) -> Result<String> {
    if uuid.is_empty() {
        return Err(Error::Validation(
            "message uuid must not be empty".to_owned(),
        ));
    }
    if uuid.chars().count() > MAX_OPENAPI_UUID_CHARS {
        return Err(Error::Validation(format!(
            "message uuid must be at most {MAX_OPENAPI_UUID_CHARS} characters"
        )));
    }
    Ok(uuid)
}

fn generate_idempotency_key() -> String {
    let pid = std::process::id();
    let sequence = NEXT_IDEMPOTENCY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    format!("lc-{pid:x}-{nanos:x}-{sequence:x}")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::Future;
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use serde_json::{Value, json};
    use url::Url;

    use super::*;
    use crate::ChannelConfig;
    use crate::lark_openapi::{BoxFuture, HttpRequest, HttpResponse};

    #[test]
    fn send_text_generates_uuid_and_reuses_it_across_transport_retry() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::transport_error("temporary network error"),
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_123"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let message_id = block_on(
            sender
                .text_message(Recipient::Chat("oc_123".to_owned()), "hello")
                .send(),
        )
        .expect("sent message after retry");

        assert_eq!(message_id, MessageId("om_123".to_owned()));

        let calls = transport.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id"
        );
        assert_eq!(calls[1].body["uuid"], calls[2].body["uuid"]);
        assert!(
            calls[1].body["uuid"]
                .as_str()
                .is_some_and(|uuid| { uuid.starts_with("lc-") && uuid.len() <= 50 })
        );
    }

    #[test]
    fn text_message_reuses_caller_uuid_across_transport_retry() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::transport_error("temporary network error"),
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_123"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let message_id = block_on(
            sender
                .text_message(Recipient::Chat("oc_123".to_owned()), "hello")
                .uuid("upstream-task-123")
                .send(),
        )
        .expect("sent message after retry");

        assert_eq!(message_id, MessageId("om_123".to_owned()));

        let calls = transport.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1].body["uuid"], "upstream-task-123");
        assert_eq!(calls[2].body["uuid"], "upstream-task-123");
    }

    #[test]
    fn message_forwards_custom_content() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_custom"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let message_id = block_on(
            sender
                .message(
                    Recipient::Chat("oc_123".to_owned()),
                    MessageContent::Custom {
                        msg_type: "custom".to_owned(),
                        content: json!({ "body": "hello" }),
                    },
                )
                .uuid("custom-uuid")
                .send(),
        )
        .expect("sent custom message");

        assert_eq!(message_id, MessageId("om_custom".to_owned()));

        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].body["msg_type"], "custom");
        assert_eq!(calls[1].body["content"], r#"{"body":"hello"}"#);
        assert_eq!(calls[1].body["uuid"], "custom-uuid");
    }

    #[test]
    fn text_message_rejects_empty_uuid() {
        let transport = FakeTransport::new(vec![]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let error = block_on(
            sender
                .text_message(Recipient::Chat("oc_123".to_owned()), "hello")
                .uuid("")
                .send(),
        )
        .expect_err("validation error");

        assert!(matches!(
            error,
            Error::Validation(message) if message == "message uuid must not be empty"
        ));
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn text_message_rejects_uuid_longer_than_fifty_characters() {
        let transport = FakeTransport::new(vec![]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let error = block_on(
            sender
                .text_message(Recipient::Chat("oc_123".to_owned()), "hello")
                .uuid("x".repeat(51))
                .send(),
        )
        .expect_err("validation error");

        assert!(matches!(
            error,
            Error::Validation(message) if message == "message uuid must be at most 50 characters"
        ));
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn send_text_does_not_retry_api_errors() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::http(
                200,
                json!({
                    "code": 99991672,
                    "msg": "missing permission"
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let error = block_on(
            sender
                .text_message(Recipient::Chat("oc_123".to_owned()), "hello")
                .send(),
        )
        .expect_err("api error");

        assert!(matches!(
            error,
            Error::Api {
                code: 99991672,
                message
            } if message == "missing permission"
        ));
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn send_text_respects_max_attempts() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::transport_error("temporary network error"),
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_123"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender =
            MessageSender::with_options(client, MessageSenderOptions::with_max_attempts(1));

        let error = block_on(
            sender
                .text_message(Recipient::Chat("oc_123".to_owned()), "hello")
                .send(),
        )
        .expect_err("transport error");

        assert!(matches!(
            error,
            Error::Transport(message) if message == "temporary network error"
        ));
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn send_text_does_not_retry_http_status_errors() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::http(
                500,
                json!({
                    "code": 0,
                    "msg": "ok"
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let error = block_on(
            sender
                .text_message(Recipient::Chat("oc_123".to_owned()), "hello")
                .send(),
        )
        .expect_err("http status error");

        assert!(matches!(error, Error::HttpStatus { status: 500 }));
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn reply_text_reuses_uuid_across_transport_retry() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::transport_error("temporary network error"),
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_reply"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let message_id = block_on(
            sender
                .text_reply(MessageId("om_parent".to_owned()), "reply")
                .send(),
        )
        .expect("replied after retry");

        assert_eq!(message_id, MessageId("om_reply".to_owned()));

        let calls = transport.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_parent/reply"
        );
        assert_eq!(calls[1].body["uuid"], calls[2].body["uuid"]);
    }

    #[test]
    fn text_reply_reuses_caller_uuid_across_transport_retry() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::transport_error("temporary network error"),
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_reply"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let message_id = block_on(
            sender
                .text_reply(MessageId("om_parent".to_owned()), "reply")
                .uuid("upstream-reply-123")
                .send(),
        )
        .expect("replied after retry");

        assert_eq!(message_id, MessageId("om_reply".to_owned()));

        let calls = transport.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1].body["uuid"], "upstream-reply-123");
        assert_eq!(calls[2].body["uuid"], "upstream-reply-123");
    }

    #[test]
    fn text_reply_forwards_thread_flag() {
        let transport = FakeTransport::new(vec![
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            FakeResponse::http(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_reply"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
        let sender = MessageSender::new(client);

        let message_id = block_on(
            sender
                .text_reply(MessageId("om_parent".to_owned()), "reply")
                .uuid("reply-uuid")
                .reply_in_thread(true)
                .send(),
        )
        .expect("replied");

        assert_eq!(message_id, MessageId("om_reply".to_owned()));

        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].body["uuid"], "reply-uuid");
        assert_eq!(calls[1].body["reply_in_thread"], true);
    }

    #[test]
    fn max_attempts_is_clamped_to_one() {
        let options = MessageSenderOptions::with_max_attempts(0);

        assert_eq!(options.max_attempts(), 1);
    }

    #[derive(Clone, Debug)]
    struct FakeTransport {
        state: Arc<Mutex<FakeState>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<FakeResponse>) -> Self {
            Self {
                state: Arc::new(Mutex::new(FakeState {
                    responses: responses.into(),
                    calls: Vec::new(),
                })),
            }
        }

        fn calls(&self) -> Vec<FakeCall> {
            self.state().calls.clone()
        }

        fn state(&self) -> MutexGuard<'_, FakeState> {
            self.state.lock().expect("fake transport state poisoned")
        }
    }

    impl OpenApiTransport for FakeTransport {
        fn send_json(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse>> {
            let response = {
                let mut state = self.state();
                state.calls.push(FakeCall {
                    url: request.url,
                    body: request.body,
                });
                state.responses.pop_front().expect("fake response")
            };

            Box::pin(async move { response.into_result() })
        }
    }

    #[derive(Debug)]
    struct FakeState {
        responses: VecDeque<FakeResponse>,
        calls: Vec<FakeCall>,
    }

    #[derive(Clone, Debug)]
    struct FakeCall {
        url: Url,
        body: Value,
    }

    #[derive(Debug)]
    enum FakeResponse {
        Http(HttpResponse),
        TransportError(String),
    }

    impl FakeResponse {
        fn http(status: u16, body: Value) -> Self {
            Self::Http(HttpResponse::json(status, body))
        }

        fn transport_error(message: impl Into<String>) -> Self {
            Self::TransportError(message.into())
        }

        fn into_result(self) -> Result<HttpResponse> {
            match self {
                Self::Http(response) => Ok(response),
                Self::TransportError(message) => Err(Error::Transport(message)),
            }
        }
    }

    impl From<HttpResponse> for FakeResponse {
        fn from(response: HttpResponse) -> Self {
            Self::Http(response)
        }
    }

    fn block_on<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    fn noop_waker() -> Waker {
        unsafe { Waker::from_raw(noop_raw_waker()) }
    }

    fn noop_raw_waker() -> RawWaker {
        fn clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }

        fn wake(_: *const ()) {}
        fn wake_by_ref(_: *const ()) {}
        fn drop(_: *const ()) {}

        RawWaker::new(
            std::ptr::null(),
            &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
        )
    }
}
