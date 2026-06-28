# Outbound Messages

This document describes the outbound message behavior currently exposed by `lark-channel`.

## Current Scope

The SDK currently provides text-message helpers and lower-level raw message entry points:

- `OpenApiClient::send_text_message`
- `OpenApiClient::send_message`
- `OpenApiClient::reply_text_message`
- `OpenApiClient::reply_message`
- `Recipient::Chat`
- `Recipient::User`

`send_text_message` is the recommended helper for plain text messages. `send_message` accepts `MessageContent` directly and can send text, raw interactive card JSON, or a custom `msg_type`/`content` payload. The SDK does not yet provide high-level builders or validation for rich message bodies.

`reply_text_message` replies to an existing message by message id. `reply_message` accepts `MessageContent` directly and follows the same raw payload rules as `send_message`.

Structured mentions, rich content builders, card helpers, media upload, retry options, and idempotency options are planned follow-up work.

## Recipients

`Recipient::Chat(chat_id)` sends to a chat container with `receive_id_type=chat_id`.

Use this when the application already has an `oc_xxx` chat id. The target can be a direct chat, group chat, or topic chat container, as long as the bot can access that chat.

`Recipient::User(open_id)` sends a direct message to a user with `receive_id_type=open_id`.

Lark/Feishu `open_id` values are scoped to the current app. An `open_id` observed from one app may fail when used by another app. Common sources for the correct app-scoped `open_id` are inbound message events, message-list sender fields, contact lookups, or other OpenAPI responses produced by the same app.

Compatibility note: the early `0.1.x` scaffold exposed `Recipient::OpenMessage` as a reply-target placeholder. Replies are now modeled explicitly with `MessageId` and `reply_message`/`reply_text_message`, so `Recipient` remains limited to new-message targets.

## Minimal Example

```rust
use lark_channel::{ChannelConfig, OpenApiClient, Recipient, ReqwestOpenApiTransport};

// Inside async application code:
let config = ChannelConfig::new("cli_xxx", "app_secret");
let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());

let message_id = client
    .send_text_message(Recipient::Chat("oc_xxx".to_owned()), "hello")
    .await?;
```

To send a direct message by user id:

```rust
let message_id = client
    .send_text_message(Recipient::User("ou_xxx".to_owned()), "hello")
    .await?;
```

To reply to an existing message:

```rust
use lark_channel::MessageId;

let message_id = client
    .reply_text_message(MessageId("om_xxx".to_owned()), "hello")
    .await?;
```

## Permissions

Sending and replying to messages require the application to have the relevant IM send permission enabled in the Lark/Feishu developer console. The bot must be able to access the conversation that contains the target message.

Message-reading permissions are separate from send permissions. For example, reading group message history requires `im:message.group_msg`, and reading group members requires a chat member read permission such as `im:chat.members:read`. Those read-side APIs are not part of the current outbound-message scope.
