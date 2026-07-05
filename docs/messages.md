# Messages

This document describes the message behavior currently exposed by `lark-channel`, including outbound text messages, replies, and minimal inbound message normalization.

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

Inbound `im.message.receive_v1` event payloads can be parsed with `parse_lark_event_payload` or `ChannelEvent::parse_lark_payload`. The current normalized message model captures the bridge-critical fields:

- message id, chat id, and chat type
- sender open id, user id, union id, and sender type when present
- message type and bridge-facing plain text when it can be derived safely
- the original Lark/Feishu stringified message content as `raw_content`
- parsed JSON message content as `content` when the original content is valid JSON
- root, parent, and thread ids when present
- structured mentions with mention key, open id, user id, union id, name, and mentioned type when provided by event metadata or supported rich-text `at` elements
- the raw event payload for unsupported or richer follow-up parsing

For `message_type=text`, `text` is read from the parsed content `text` field. Mention placeholder keys such as `@_user_1` are replaced with `@name` when the event metadata provides a matching mention name. For `message_type=post`, `text` is derived from the selected post document title and supported inline elements. The current post normalization chooses `zh_cn`, then `en_us`, then `ja_jp`, then the first document-shaped locale block. It includes `text`, link text, and `@user_name` from `at` elements, joins post lines with newlines, and skips non-text resource elements such as images while keeping the full parsed `content` available.

Mentions from event metadata are treated as the authoritative source when present. Rich-text `at` elements can add mention entries when they expose a user identifier and name; duplicate entries are merged by key, open id, user id, or union id. Event mention ids are accepted in both the nested `id.open_id/user_id/union_id` shape and the `id` plus `id_type` shape used by some generated models. `@all` rich-text mentions are preserved with the key `@_all` when exposed by Lark/Feishu.

Malformed message content does not drop an otherwise valid receive event. In that case `raw_content` preserves the exact content string, `content` is `None`, and `text` is empty. Unsupported message types still produce `ChannelEvent::Message` with message metadata and raw payload access; richer normalization remains follow-up work.

Use `NormalizedMessage::mentions_bot(bot_open_id)` to decide whether a group message explicitly mentions the current bot. Media/resource descriptors and advanced rich-content rendering remain later normalization work.

Card action callback payloads with event type `card.action.trigger` are parsed as `ChannelEvent::CardAction`. The current model exposes the operator ids, callback update token, action value, form/input/select values, host metadata, open message id, open chat id, and raw payload. Responding to a card callback or updating the card content remains later card-helper work.

With the `websocket` feature enabled, `EventConsumer` can receive a WebSocket event, parse it into `ChannelEvent`, call a user-provided handler, and ACK the underlying event frame. `handle_next_event` adds `biz_rt` when the handler ACK omits it. If event parsing fails after a complete event is received, `EventConsumer` attempts to send an internal-server-error ACK before returning. Handler errors are also ACKed as internal-server-error before the original handler error is returned to the caller, matching the official SDK long-connection behavior.

`EventLoop` adds a reconnecting receive loop with the same parse, handler, and ACK semantics as `EventConsumer`. It reconnects after clean socket closes and transport errors, requests a fresh WebSocket endpoint through `OpenApiWebSocketEventConnector`, and returns `EventLoopExit::ReconnectLimitReached` when the effective reconnect limit is reached after clean closes. If the final retryable failure is a transport error, it returns that error. By default, the loop follows endpoint `ClientConfig` reconnect policy: `ReconnectCount=-1` means unlimited retries, non-negative `ReconnectCount` values are finite retry counts, and `ReconnectInterval`/`ReconnectNonce` are used only when they are positive durations. Explicit `with_max_reconnects`, `with_unlimited_reconnects`, or `with_reconnect_delay` options select a local reconnect policy instead. `with_server_reconnect_config(true)` can re-enable server-provided reconnect policy after local fallback values are configured.

The loop sends the official application-level heartbeat ping at the endpoint-provided `PingInterval` while waiting for the next event, falling back to 120 seconds when the endpoint omits a positive interval; heartbeat send failures are treated as reconnectable transport errors. `EventLoopOptions::with_heartbeat_timeout` can additionally mark the connection dead if no application-level activity is observed shortly after a heartbeat ping sent while waiting for events. Internally, `EventLoop` keeps receive/reassembly, handler dispatch/ACK policy, and protocol writes as separate runtime responsibilities, but they still run over one mutable connection. User handlers are awaited without connection heartbeat work, so long-running work should be spawned outside the event loop and ACKed according to the caller's delivery policy.

`EventConsumer` and `EventLoop` reassemble Lark/Feishu application-level split packets whose protocol metadata reports `sum > 1` before parsing the payload or invoking handlers. Packet sequence values are zero-based for multi-packet events; duplicate packet sequences replace the earlier packet bytes. Reassembly uses bounded defaults of 1024 parts, 16 MiB per logical event, and 128 pending logical events. These defaults can be tuned with `EventPacketReassemblyOptions` through `EventConsumer::with_reassembly_options` or `EventLoopOptions::with_reassembly_options`. If a packet is malformed or exceeds the configured limits, the high-level consumer attempts an internal-server-error ACK for that packet and returns the validation error. Lower-level `WebSocketConnection::next_event` still exposes raw event frames with their original `sum` and `seq` values for callers that need direct protocol control.

Lower-level raw message entry points are available under `lark_channel::lark_openapi` for callers that need to pass `MessageContent` directly. See [lark-api.md](lark-api.md) for the exact official API mappings.

Rich mention composition, rich content builders, card helpers, media upload, and richer retry policies are planned follow-up work.

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
