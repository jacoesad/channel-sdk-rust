# Messages

This document describes the message behavior currently exposed by `lark-channel`, starting with outbound text messages and replies.

## Current Scope

The SDK currently provides a high-level `MessageSender` for text messages and replies:

- `MessageSender::message`
- `MessageSender::text_message`
- `MessageSender::reply`
- `MessageSender::text_reply`
- `MessageSenderOptions`
- `MessageBuilder`
- `MessageReplyBuilder`

`message` and `reply` accept caller-provided `MessageContent`. `text_message` and `text_reply` are convenience entry points for plain text content.

`MessageSender` automatically generates one idempotency key per logical send or reply and reuses it across conservative transport-failure retries. Callers that already have a stable upstream request, task, or event identifier can provide it through the per-call options. Caller-provided `uuid` values must be non-empty and at most 50 characters. `MessageSender` does not retry API errors, validation failures, or OpenAPI HTTP status errors.

Lower-level raw message entry points are available under `lark_channel::lark_openapi` for callers that need to pass `MessageContent` directly. See [lark-api.md](lark-api.md) for the exact official API mappings.

Structured mentions, rich content builders, card helpers, media upload, and richer retry policies are planned follow-up work.

Runnable examples are documented in [../examples/README.md](../examples/README.md), including low-level create/reply calls and the high-level `MessageSender` flow.

## Recipients

`Recipient::Chat(chat_id)` sends to a chat container with `receive_id_type=chat_id`.

Use this when the application already has an `oc_xxx` chat id. The target can be a direct chat, group chat, or topic chat container, as long as the bot can access that chat.

`Recipient::User(open_id)` sends a direct message to a user with `receive_id_type=open_id`.

Lark/Feishu `open_id` values are scoped to the current app. An `open_id` observed from one app may fail when used by another app. Common sources for the correct app-scoped `open_id` are inbound message events, message-list sender fields, contact lookups, or other OpenAPI responses produced by the same app.

## Minimal Example

```rust
use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ChannelConfig, MessageSender, Recipient};

// Inside async application code:
let config = ChannelConfig::new("cli_xxx", "app_secret");
let openapi = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
let sender = MessageSender::new(openapi);

let message_id = sender
    .text_message(Recipient::Chat("oc_xxx".to_owned()), "hello")
    .send()
    .await?;
```

To send a direct message by user id:

```rust
let message_id = sender
    .text_message(Recipient::User("ou_xxx".to_owned()), "hello")
    .send()
    .await?;
```

To reply to an existing message:

```rust
use lark_channel::MessageId;

let message_id = sender
    .text_reply(MessageId("om_xxx".to_owned()), "hello")
    .send()
    .await?;
```

To provide a stable upstream de-duplication key:

```rust
let message_id = sender
    .text_message(Recipient::Chat("oc_xxx".to_owned()), "hello")
    .uuid("upstream-task-123")
    .send()
    .await?;
```

Reply calls can also set `reply_in_thread` through per-call options:

```rust
let message_id = sender
    .text_reply(MessageId("om_xxx".to_owned()), "hello")
    .uuid("reply-task-123")
    .reply_in_thread(true)
    .send()
    .await?;
```

To tune sender retry attempts:

```rust
use lark_channel::MessageSenderOptions;

let sender = MessageSender::with_options(
    openapi,
    MessageSenderOptions::with_max_attempts(2),
);
```

Per-call retry attempts can also be tuned on the operation builder:

```rust
let message_id = sender
    .text_message(Recipient::Chat("oc_xxx".to_owned()), "hello")
    .max_attempts(1)
    .send()
    .await?;
```

Lark/Feishu uses `uuid` for request de-duplication. In a short-window smoke test, sending the same message twice with the same `uuid` returned the same `message_id` instead of creating a second message or returning a duplicate error. The official OpenAPI documentation states that requests with the same `uuid` can succeed at most once within one hour, and the value can be up to 50 characters.

`MessageSender` follows the same idempotency rule used by the Python channel SDK: choose a `uuid` once for a logical send or reply, then reuse that value across every internal retry attempt. By default it generates that value automatically; per-call options let callers provide a stable upstream value. If the content or target changes, use a new `uuid`.

## Permissions

Sending and replying to messages require the application to have the relevant IM send permission enabled in the Lark/Feishu developer console. The bot must be able to access the conversation that contains the target message.

Message-reading permissions are separate from send permissions. For example, reading group message history requires `im:message.group_msg`, and reading group members requires a chat member read permission such as `im:chat.members:read`. Those read-side APIs are not part of the current message scope.
