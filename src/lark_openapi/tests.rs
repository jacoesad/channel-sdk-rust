use serde_json::{Value, json};

use super::test_support::{FakeTransport, block_on};
use super::*;
use crate::message::{MessageContent, MessageId, PostContent, Recipient};
use crate::{ChannelConfig, Error};

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
fn create_message_preserves_raw_template_card_content() {
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
                "data": { "message_id": "om_template" }
            }),
        ),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let template = json!({
        "type": "template",
        "data": {
            "template_id": "AAq_example",
            "template_variable": { "name": "Rust" }
        }
    });

    let message_id = block_on(client.create_message(
        Recipient::Chat("oc_123".to_owned()),
        MessageContent::Card {
            card: template.clone(),
        },
    ))
    .expect("sent template card");

    assert_eq!(message_id, MessageId("om_template".to_owned()));
    let body = &transport.calls()[1].body;
    assert_eq!(body["msg_type"], "interactive");
    let content: Value = serde_json::from_str(body["content"].as_str().expect("content string"))
        .expect("template card json");
    assert_eq!(content, template);
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
fn reply_message_preserves_raw_template_card_content() {
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
                "data": { "message_id": "om_template_reply" }
            }),
        ),
    ]);
    let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());
    let template = json!({
        "type": "template",
        "data": {
            "template_id": "AAq_example",
            "template_version_name": "1.0.0"
        }
    });

    let message_id = block_on(client.reply_message(
        MessageId("om_parent".to_owned()),
        MessageContent::Card {
            card: template.clone(),
        },
    ))
    .expect("replied with template card");

    assert_eq!(message_id, MessageId("om_template_reply".to_owned()));
    let body = &transport.calls()[1].body;
    assert_eq!(body["msg_type"], "interactive");
    let content: Value = serde_json::from_str(body["content"].as_str().expect("content string"))
        .expect("template card json");
    assert_eq!(content, template);
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
