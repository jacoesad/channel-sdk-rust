use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use serde_json::{Value, json};
use url::Url;

use super::*;
use crate::message::{MessageContent, MessageId, Recipient};
use crate::{ChannelConfig, Error, Result};

#[test]
fn app_access_token_requests_and_caches_token() {
    let transport = FakeTransport::new(vec![HttpResponse::json(
        200,
        json!({
            "code": 0,
            "msg": "ok",
            "app_access_token": "token-1",
            "expire": 7200
        }),
    )]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

    let first = block_on(client.app_access_token()).expect("first token");
    let second = block_on(client.app_access_token()).expect("cached token");

    assert_eq!(first, "token-1");
    assert_eq!(second, "token-1");

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].url.as_str(),
        "https://open.feishu.cn/open-apis/auth/v3/app_access_token/internal"
    );
    assert_eq!(
        calls[0].body,
        json!({
            "app_id": "cli_a",
            "app_secret": "secret"
        })
    );
}

#[test]
fn app_access_token_refreshes_when_token_is_inside_refresh_skew() {
    let transport = FakeTransport::new(vec![
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "app_access_token": "token-1",
                "expire": 1
            }),
        ),
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "app_access_token": "token-2",
                "expire": 7200
            }),
        ),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

    let first = block_on(client.app_access_token()).expect("first token");
    let second = block_on(client.app_access_token()).expect("refreshed token");

    assert_eq!(first, "token-1");
    assert_eq!(second, "token-2");
    assert_eq!(transport.calls().len(), 2);
}

#[test]
fn tenant_access_token_requests_and_caches_token() {
    let transport = FakeTransport::new(vec![HttpResponse::json(
        200,
        json!({
            "code": 0,
            "msg": "ok",
            "tenant_access_token": "tenant-token-1",
            "expire": 7200
        }),
    )]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

    let first = block_on(client.tenant_access_token()).expect("first tenant token");
    let second = block_on(client.tenant_access_token()).expect("cached tenant token");

    assert_eq!(first, "tenant-token-1");
    assert_eq!(second, "tenant-token-1");

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].url.as_str(),
        "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal"
    );
    assert_eq!(
        calls[0].body,
        json!({
            "app_id": "cli_a",
            "app_secret": "secret"
        })
    );
}

#[test]
fn post_tenant_json_adds_bearer_token() {
    let transport = FakeTransport::new(vec![
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        ),
        HttpResponse::json(
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

    let response = block_on(client.post_tenant_json::<_, Value>(
        "/open-apis/im/v1/messages",
        &json!({
            "receive_id": "oc_123",
            "msg_type": "text"
        }),
    ))
    .expect("tenant request");

    assert_eq!(
        response,
        json!({
            "code": 0,
            "msg": "ok",
            "data": {
                "message_id": "om_123"
            }
        })
    );

    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].url.as_str(),
        "https://open.feishu.cn/open-apis/im/v1/messages"
    );
    assert_eq!(
        calls[1].headers.get("authorization").map(String::as_str),
        Some("Bearer tenant-token-1")
    );
    assert_eq!(
        calls[1].headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
}

#[test]
fn create_message_posts_tenant_message() {
    let transport = FakeTransport::new(vec![
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        ),
        HttpResponse::json(
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

    let message_id = block_on(client.create_message(
        Recipient::Chat("oc_123".to_owned()),
        MessageContent::Text {
            text: "hello from rust".to_owned(),
        },
    ))
    .expect("sent message");

    assert_eq!(message_id, MessageId("om_123".to_owned()));

    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].url.as_str(),
        "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id"
    );
    assert_eq!(
        calls[1].headers.get("authorization").map(String::as_str),
        Some("Bearer tenant-token-1")
    );
    assert_eq!(
        calls[1].body,
        json!({
            "receive_id": "oc_123",
            "msg_type": "text",
            "content": "{\"text\":\"hello from rust\"}"
        })
    );
}

#[test]
fn create_message_maps_user_recipient_to_open_id() {
    let transport = FakeTransport::new(vec![
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        ),
        HttpResponse::json(
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

    block_on(client.create_message(
        Recipient::User("ou_123".to_owned()),
        MessageContent::Custom {
            msg_type: "text".to_owned(),
            content: json!({ "text": "hello" }),
        },
    ))
    .expect("sent message");

    let calls = transport.calls();
    assert_eq!(
        calls[1].url.as_str(),
        "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=open_id"
    );
    assert_eq!(
        calls[1].body,
        json!({
            "receive_id": "ou_123",
            "msg_type": "text",
            "content": "{\"text\":\"hello\"}"
        })
    );
}

#[test]
fn create_message_with_options_includes_uuid() {
    let transport = FakeTransport::new(vec![
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        ),
        HttpResponse::json(
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

    block_on(client.create_message_with_options(
        Recipient::Chat("oc_123".to_owned()),
        MessageContent::Text {
            text: "hello".to_owned(),
        },
        MessageCreateOptions::with_uuid("uuid-123"),
    ))
    .expect("sent message");

    let calls = transport.calls();
    assert_eq!(
        calls[1].body,
        json!({
            "receive_id": "oc_123",
            "msg_type": "text",
            "content": "{\"text\":\"hello\"}",
            "uuid": "uuid-123"
        })
    );
}

#[test]
fn reply_message_posts_tenant_reply() {
    let transport = FakeTransport::new(vec![
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        ),
        HttpResponse::json(
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

    let message_id = block_on(client.reply_message(
        MessageId("om_parent".to_owned()),
        MessageContent::Text {
            text: "reply from rust".to_owned(),
        },
    ))
    .expect("replied to message");

    assert_eq!(message_id, MessageId("om_reply".to_owned()));

    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].url.as_str(),
        "https://open.feishu.cn/open-apis/im/v1/messages/om_parent/reply"
    );
    assert_eq!(
        calls[1].headers.get("authorization").map(String::as_str),
        Some("Bearer tenant-token-1")
    );
    assert_eq!(
        calls[1].body,
        json!({
            "msg_type": "text",
            "content": "{\"text\":\"reply from rust\"}"
        })
    );
}

#[test]
fn reply_message_with_options_includes_uuid_and_thread_flag() {
    let transport = FakeTransport::new(vec![
        HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        ),
        HttpResponse::json(
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

    block_on(client.reply_message_with_options(
        MessageId("om_parent".to_owned()),
        MessageContent::Text {
            text: "reply from rust".to_owned(),
        },
        MessageReplyOptions::with_uuid("uuid-reply").reply_in_thread(true),
    ))
    .expect("replied to message");

    let calls = transport.calls();
    assert_eq!(
        calls[1].body,
        json!({
            "msg_type": "text",
            "content": "{\"text\":\"reply from rust\"}",
            "uuid": "uuid-reply",
            "reply_in_thread": true
        })
    );
}

#[test]
fn post_openapi_json_returns_typed_api_error() {
    let transport = FakeTransport::new(vec![HttpResponse::json(
        200,
        json!({
            "code": 99991663,
            "msg": "invalid app secret"
        }),
    )]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

    let error = block_on(client.app_access_token()).expect_err("api error");

    assert!(matches!(
        error,
        Error::Api {
            code: 99991663,
            message
        } if message == "invalid app secret"
    ));
}

#[test]
fn post_openapi_json_accepts_message_alias_for_api_error() {
    let transport = FakeTransport::new(vec![HttpResponse::json(
        200,
        json!({
            "code": 99991663,
            "message": "invalid app secret"
        }),
    )]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

    let error = block_on(client.app_access_token()).expect_err("api error");

    assert!(matches!(
        error,
        Error::Api {
            code: 99991663,
            message
        } if message == "invalid app secret"
    ));
}

#[test]
fn post_openapi_json_returns_http_status_error_for_non_success_status() {
    let transport = FakeTransport::new(vec![HttpResponse::json(
        500,
        json!({
            "code": 0,
            "msg": "ok"
        }),
    )]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

    let error = block_on(client.post_openapi_json::<_, Value>("/open-apis/example", &json!({})))
        .expect_err("http status error");

    assert!(matches!(error, Error::HttpStatus { status: 500 }));
}

#[derive(Clone, Debug)]
struct FakeTransport {
    state: Arc<Mutex<FakeState>>,
}

impl FakeTransport {
    fn new(responses: Vec<HttpResponse>) -> Self {
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
                headers: request.headers,
                body: request.body,
            });
            state.responses.pop_front().expect("fake response")
        };

        Box::pin(async move { Ok(response) })
    }
}

#[derive(Debug)]
struct FakeState {
    responses: VecDeque<HttpResponse>,
    calls: Vec<FakeCall>,
}

#[derive(Clone, Debug)]
struct FakeCall {
    url: Url,
    headers: BTreeMap<String, String>,
    body: Value,
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
