use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use serde_json::{Value, json};
use url::Url;

use super::*;
use crate::message::{MessageContent, MessageId, PostContent, Recipient};
use crate::{Card, CardId, ChannelConfig, Error, Result};

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
fn update_message_card_patches_serialized_shared_card() {
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
        HttpResponse::json(200, json!({ "code": 0, "msg": "ok", "data": {} })),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let card = Card::builder().markdown("updated").build().expect("card");

    block_on(client.update_message_card(&MessageId("om_123".to_owned()), &card))
        .expect("updated message card");

    let calls = transport.calls();
    assert_eq!(calls[1].method, HttpMethod::Patch);
    assert_eq!(
        calls[1].url.as_str(),
        "https://open.feishu.cn/open-apis/im/v1/messages/om_123"
    );
    let content: Value =
        serde_json::from_str(calls[1].body["content"].as_str().expect("content string"))
            .expect("card json");
    assert_eq!(content["schema"], "2.0");
    assert_eq!(content["config"]["update_multi"], true);
    assert_eq!(content["body"]["elements"][0]["content"], "updated");
}

#[test]
fn update_message_card_rejects_url_path_delimiters_before_authentication() {
    let transport = FakeTransport::new(vec![]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let card = Card::builder().text("hello").build().expect("card");

    for message_id in ["om_1/extra", "om_1?x=1", "om_1#fragment", "om_1%2Fextra"] {
        let error = block_on(client.update_message_card(&MessageId(message_id.to_owned()), &card))
            .expect_err("unsafe message_id must fail");
        assert!(matches!(error, Error::Validation(_)));
    }
    assert!(transport.calls().is_empty());
}

#[test]
fn create_card_entity_posts_card_json_and_validates_response_id() {
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
                "data": { "card_id": "7355372766134157313" }
            }),
        ),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let card = Card::builder().text("hello").build().expect("card");

    let card_id = block_on(client.create_card_entity(&card)).expect("card entity");

    assert_eq!(card_id, CardId("7355372766134157313".to_owned()));
    let calls = transport.calls();
    assert_eq!(calls[1].method, HttpMethod::Post);
    assert_eq!(
        calls[1].url.as_str(),
        "https://open.feishu.cn/open-apis/cardkit/v1/cards"
    );
    assert_eq!(calls[1].body["type"], "card_json");
    let data: Value = serde_json::from_str(calls[1].body["data"].as_str().expect("data string"))
        .expect("card json");
    assert_eq!(data["schema"], "2.0");
    assert_eq!(data["body"]["elements"][0]["text"]["content"], "hello");
}

#[test]
fn update_card_entity_puts_sequence_uuid_and_card_json() {
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
        HttpResponse::json(200, json!({ "code": 0, "msg": "ok", "data": {} })),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let card = Card::builder().markdown("done").build().expect("card");
    let card_id = CardId::new("7355372766134157313").expect("card id");

    block_on(client.update_card_entity(
        &card_id,
        &card,
        CardUpdateOptions::new(2).uuid("card-update-2"),
    ))
    .expect("updated card entity");

    let calls = transport.calls();
    assert_eq!(calls[1].method, HttpMethod::Put);
    assert_eq!(
        calls[1].url.as_str(),
        "https://open.feishu.cn/open-apis/cardkit/v1/cards/7355372766134157313"
    );
    assert_eq!(calls[1].body["sequence"], 2);
    assert_eq!(calls[1].body["uuid"], "card-update-2");
    assert_eq!(calls[1].body["card"]["type"], "card_json");
    let data: Value =
        serde_json::from_str(calls[1].body["card"]["data"].as_str().expect("data string"))
            .expect("card json");
    assert_eq!(data["body"]["elements"][0]["content"], "done");
}

#[test]
fn update_card_entity_rejects_invalid_sequence_before_authentication() {
    let transport = FakeTransport::new(vec![]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let card = Card::builder().text("hello").build().expect("card");
    let card_id = CardId::new("7355372766134157313").expect("card id");

    let error = block_on(client.update_card_entity(&card_id, &card, CardUpdateOptions::new(0)))
        .expect_err("zero sequence must fail");

    assert!(matches!(
        error,
        Error::Validation(message) if message.contains("card update sequence")
    ));
    assert!(transport.calls().is_empty());
}

#[test]
fn update_card_entity_rejects_invalid_uuid_before_authentication() {
    let transport = FakeTransport::new(vec![]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let card = Card::builder().text("hello").build().expect("card");
    let card_id = CardId::new("7355372766134157313").expect("card id");

    for uuid in [String::new(), "x".repeat(65)] {
        let error = block_on(client.update_card_entity(
            &card_id,
            &card,
            CardUpdateOptions::new(1).uuid(uuid),
        ))
        .expect_err("invalid card update uuid must fail");
        assert!(matches!(error, Error::Validation(_)));
    }
    assert!(transport.calls().is_empty());
}

#[test]
fn create_message_serializes_card_entity_reference() {
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
                "data": { "message_id": "om_card" }
            }),
        ),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

    let message_id = block_on(client.create_message(
        Recipient::Chat("oc_123".to_owned()),
        MessageContent::CardReference {
            card_id: CardId::new("7355372766134157313").expect("card id"),
        },
    ))
    .expect("card reference message");

    assert_eq!(message_id, MessageId("om_card".to_owned()));
    let body = &transport.calls()[1].body;
    assert_eq!(body["msg_type"], "interactive");
    let content: Value = serde_json::from_str(body["content"].as_str().expect("content string"))
        .expect("card reference json");
    assert_eq!(
        content,
        json!({ "type": "card", "data": { "card_id": "7355372766134157313" } })
    );
}

#[test]
fn create_message_rejects_invalid_raw_card_before_authentication() {
    let transport = FakeTransport::new(vec![]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

    let error = block_on(client.create_message(
        Recipient::Chat("oc_123".to_owned()),
        MessageContent::Card {
            card: json!({
                "schema": "1.0",
                "body": { "elements": [{ "tag": "markdown", "content": "hello" }] }
            }),
        },
    ))
    .expect_err("non-2.0 card must fail");

    assert!(matches!(
        error,
        Error::Validation(message) if message == "card schema must be \"2.0\""
    ));
    assert!(transport.calls().is_empty());
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
fn create_message_serializes_native_markdown_post_content() {
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
                    "message_id": "om_post"
                }
            }),
        ),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

    let message_id = block_on(client.create_message(
        Recipient::Chat("oc_123".to_owned()),
        MessageContent::Post {
            post: PostContent::markdown("**hello** [docs](https://open.feishu.cn)"),
        },
    ))
    .expect("sent post message");

    assert_eq!(message_id, MessageId("om_post".to_owned()));
    assert_eq!(
        transport.calls()[1].body,
        json!({
            "receive_id": "oc_123",
            "msg_type": "post",
            "content": "{\"zh_cn\":{\"content\":[[{\"tag\":\"md\",\"text\":\"**hello** [docs](https://open.feishu.cn)\"}]]}}"
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
fn reply_message_rejects_url_path_delimiters_before_authentication() {
    let transport = FakeTransport::new(vec![]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

    for message_id in ["om_1/extra", "om_1?x=1", "om_1#fragment", "om_1%2Fextra"] {
        let error = block_on(client.reply_message(
            MessageId(message_id.to_owned()),
            MessageContent::Text {
                text: "hello".to_owned(),
            },
        ))
        .expect_err("unsafe parent message_id must fail");
        assert!(matches!(error, Error::Validation(_)));
    }
    assert!(transport.calls().is_empty());
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
                method: request.method,
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
    method: HttpMethod,
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
