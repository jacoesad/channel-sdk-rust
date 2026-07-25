# Messages

This document describes the message behavior currently exposed by `lark-channel`, including outbound text, rich-text, and media messages, high-level Markdown streaming, replies, and inbound message normalization.

## Current Scope

The SDK currently provides a high-level `MessageSender` for text, rich-text, media, and streaming Markdown messages and replies:

- `MessageSender::message`
- `MessageSender::text_message`
- `MessageSender::post_message`
- `MessageSender::markdown_message`
- `MessageSender::image_message`
- `MessageSender::file_message`
- `MessageSender::audio_message`
- `MessageSender::media_message`
- `MessageSender::card_message`
- `MessageSender::card_reference_message`
- `MessageSender::reply`
- `MessageSender::text_reply`
- `MessageSender::post_reply`
- `MessageSender::markdown_reply`
- `MessageSender::image_reply`
- `MessageSender::file_reply`
- `MessageSender::audio_reply`
- `MessageSender::media_reply`
- `MessageSender::card_reply`
- `MessageSender::card_reference_reply`
- `MessageSender::markdown_stream_message`
- `MessageSender::markdown_stream_reply`
- `MessageSenderOptions`
- `MessageBuilder`
- `MessageReplyBuilder`
- `MarkdownStreamBuilder`
- `MarkdownStream`
- `ThrottledMarkdownStream`

`message` and `reply` accept caller-provided `MessageContent`. `MessageContent::Card` remains the raw `interactive` escape hatch for official payloads such as template cards. `text_message` and `text_reply` are convenience entry points for plain text content. `post_message` and `post_reply` accept typed `PostContent`; `markdown_message` and `markdown_reply` wrap Markdown in native rich-text content automatically. Image, file, audio, and media helpers accept resource keys returned by the official upload APIs. `media_message` and `media_reply` use the official `media` type for MP4 video and accept an optional uploaded image key as the cover. `card_message` and `card_reply` accept a validated `Card` and send inline CardKit 2.0 JSON, while the card-reference variants send a pre-created `CardId`.

`PostContent::markdown` creates the official `post` shape with one `tag=md` element. Lark/Feishu renders the content according to the native Markdown syntax supported by the current platform and client, so the SDK does not maintain a separate Markdown parser. Consult the official message-content documentation for the current syntax and client-version limitations. `PostContent::text` creates a structured plain-text post, and `PostContentBuilder` can select the documented `zh_cn` or `en_us` locale, set a title, or append multiple Markdown and structured paragraphs.

Structured paragraphs use `PostElement` helpers for text, the optional boolean `un_escape` text flag, validated links, @user/@all mentions, and supported `PostStyle` values. Native `md` elements must occupy their own paragraph. Use `MessageContent::Custom` as the lower-level escape hatch for official post elements or future locale values that are not modeled yet.

`MessageContent` is non-exhaustive. Downstream matches must include a wildcard arm so future message content types can be added without another source-breaking enum change. Its serde representation is not forward-compatible with unknown future variants: when persisted data or mixed-version deployments are involved, upgrade readers before writers. The `Post` variant was introduced in the `v0.5.0` release line, and the media content variants were introduced in the `v0.6.0` release line.

The `v0.5.0` card API intentionally replaces the initial scaffold's public-field `Card` struct with an opaque, validated CardKit 2.0 value. Replace `Card { schema, body }` construction with `Card::builder()`, `Card::from_value(full_card_json)`, or `Card::raw(body_json)`, and propagate the returned `Result`. This is a source-breaking pre-1.0 migration; serialized CardKit JSON remains the official schema 2.0 shape.

`MessageSender` automatically generates one idempotency key per logical send or reply and reuses it across conservative transport-failure retries. Callers that already have a stable upstream request, task, or event identifier can provide it through the per-call options. Caller-provided `uuid` values must be non-empty and at most 50 characters. `MessageSender` does not retry API errors, validation failures, or OpenAPI HTTP status errors.

Inbound `im.message.receive_v1` event payloads can be parsed with `parse_lark_event_payload` or `ChannelEvent::parse_lark_payload`. The current normalized message model captures the bridge-critical fields:

- message id, chat id, and chat type
- sender open id, user id, union id, and sender type when present
- message type and bridge-facing plain text when it can be derived safely
- the original Lark/Feishu stringified message content as `raw_content`
- parsed JSON message content as `content` when the original content is valid JSON
- root, parent, and thread ids when present
- structured mentions with mention key, open id, user id, union id, name, and mentioned type when provided by event metadata or supported rich-text `at` elements
- lightweight resource descriptors for supported image, file, folder, audio, video, sticker, and rich-text resource elements
- the raw event payload for unsupported or richer follow-up parsing

For `message_type=text`, `text` is read from the parsed content `text` field. Mention placeholder keys such as `@_user_1` are replaced with `@name` when the event metadata provides a matching mention name. For `message_type=post`, `text` is derived from the selected post document title and supported inline elements. The current post normalization chooses `zh_cn`, then `en_us`, then `ja_jp`, then the first document-shaped locale block. It includes `text`, link text, and `@user_name` from `at` elements, joins post lines with newlines, and skips non-text resource elements such as images while keeping the full parsed `content` available.

Mentions from event metadata are treated as the authoritative source when present. Rich-text `at` elements can add mention entries when they expose a user identifier and name; duplicate entries are merged by key, open id, user id, or union id. Event mention ids are accepted in both the nested `id.open_id/user_id/union_id` shape and the `id` plus `id_type` shape used by some generated models. `@all` rich-text mentions are preserved with the key `@_all` when exposed by Lark/Feishu.

Malformed message content does not drop an otherwise valid receive event. In that case `raw_content` preserves the exact content string, `content` is `None`, and `text` is empty. Unsupported message types still produce `ChannelEvent::Message` with message metadata and raw payload access; richer normalization remains follow-up work.

Use `NormalizedMessage::mentions_bot(bot_open_id)` to decide whether a group message explicitly mentions the current bot.

`NormalizedMessage::resources` contains lightweight `ResourceDescriptor` values when a supported message content shape exposes a resource key:

- `image`: `image_key`
- `file` and `folder`: `file_key` and `file_name`
- `audio`: `file_key` and `duration_ms`
- `media`: `file_key`, optional cover `image_key`, `file_name`, and `duration_ms`
- `sticker`: `file_key`
- `post`: embedded `img` and `media` elements from `content` or `content_v2`

Use `MediaDownloader` to download supported descriptors into memory. Images map to the official `image` resource type; files, audio, and video map to `file`. Folder, sticker, and unknown descriptors are rejected before authentication because the official message-resource endpoint does not support them. See [media.md](media.md) for download behavior and remaining media scope.

Card action callback payloads with event type `card.action.trigger` are parsed as `ChannelEvent::CardAction`. The current model exposes the operator ids, callback update token, action value, form/input/select values, host metadata, open message id, open chat id, and raw payload. `CardActionResponse` builds an immediate empty, Toast, or CardKit 2.0 card response and converts it into the Base64 JSON data required by a WebSocket ACK.

For delayed updates, first return a successful callback ACK, then pass the callback token to `OpenApiClient::update_message_card_with_callback_token` from work that runs after the ACK. The token is valid for 30 minutes and can be used at most twice. Calling the delayed update concurrently with or before the ACK can fail or be reverted by the platform, so the SDK intentionally does not hide this ordering behind an automatic request.

With the `websocket` feature enabled, `EventConsumer` can receive a WebSocket event, parse it into `ChannelEvent`, call a user-provided handler, and ACK the underlying event frame. `handle_next_event` adds `biz_rt` when the handler ACK omits it. If event parsing fails after a complete event is received, `EventConsumer` attempts to send an internal-server-error ACK before returning. Handler errors are also ACKed as internal-server-error before the original handler error is returned to the caller, matching the official SDK long-connection behavior.

`EventLoop` adds a reconnecting receive loop with the same parse, handler, and ACK semantics as `EventConsumer`. It reconnects after clean socket closes and transport errors, requests a fresh WebSocket endpoint through `OpenApiWebSocketEventConnector`, and returns `EventLoopExit::ReconnectLimitReached` when the effective reconnect limit is reached after clean closes. If the final retryable failure is a transport error, it returns that error. By default, the loop follows endpoint `ClientConfig` reconnect policy: `ReconnectCount=-1` means unlimited retries, non-negative `ReconnectCount` values are finite retry counts, and `ReconnectInterval`/`ReconnectNonce` are used only when they are positive durations. Explicit `with_max_reconnects`, `with_unlimited_reconnects`, or `with_reconnect_delay` options select a local reconnect policy instead. `with_server_reconnect_config(true)` can re-enable server-provided reconnect policy after local fallback values are configured.

The loop sends the official application-level heartbeat ping at the endpoint-provided `PingInterval` while waiting for the next event, falling back to 120 seconds when the endpoint omits a positive interval; heartbeat send failures are treated as reconnectable transport errors. `EventLoopOptions::with_heartbeat_timeout` can additionally mark the connection dead if no application-level activity is observed shortly after a heartbeat ping sent while waiting for events. Internally, `EventLoop` keeps receive/reassembly, handler dispatch/ACK policy, and protocol writes as separate runtime responsibilities, but they still run over one mutable connection. User handlers are awaited without connection heartbeat work, so long-running work should be spawned outside the event loop and ACKed according to the caller's delivery policy.

`EventConsumer` and `EventLoop` reassemble Lark/Feishu application-level split packets whose protocol metadata reports `sum > 1` before parsing the payload or invoking handlers. Packet sequence values are zero-based for multi-packet events; duplicate packet sequences replace the earlier packet bytes. Reassembly uses bounded defaults of 1024 parts, 16 MiB per logical event, and 128 pending logical events. These defaults can be tuned with `EventPacketReassemblyOptions` through `EventConsumer::with_reassembly_options` or `EventLoopOptions::with_reassembly_options`. If a packet is malformed or exceeds the configured limits, the high-level consumer attempts an internal-server-error ACK for that packet and returns the validation error. Lower-level `WebSocketConnection::next_event` still exposes raw event frames with their original `sum` and `seq` values for callers that need direct protocol control.

Lower-level raw message entry points are available under `lark_channel::lark_openapi` for callers that need to pass `MessageContent` directly. See [lark-api.md](lark-api.md) for the exact official API mappings.

Media uploads and messages are documented in [media.md](media.md). The low-level CardKit streaming calls are documented in [lark-api.md](lark-api.md).

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

To send native Markdown rich text:

```rust
let message_id = sender
    .markdown_message(
        Recipient::Chat("oc_xxx".to_owned()),
        "## Build status\n\n- **Passed**\n- [Details](https://example.com)",
    )
    .send()
    .await?;
```

To compose structured links and mentions:

```rust
use lark_channel::{PostContent, PostElement, PostStyle};

let post = PostContent::builder()
    .title("Build status")
    .paragraph([
        PostElement::mention("ou_xxx")?,
        PostElement::text(" see "),
        PostElement::link("details", "https://example.com")?
            .with_styles([PostStyle::Bold])?,
    ])
    .build()?;

let message_id = sender
    .post_message(Recipient::Chat("oc_xxx".to_owned()), post)
    .send()
    .await?;
```

To build and send a CardKit 2.0 card:

```rust
use lark_channel::{Card, CardButtonStyle, CardElement};
use serde_json::json;

let approve = CardElement::callback_button(
    "Approve",
    json!({ "choice": "approve" }),
)?
.button_style(CardButtonStyle::Primary)?
.element_id("approve_button")?;

let card = Card::builder()
    .header("Deployment")
    .header_template("blue")
    .markdown("Production is **ready**.")
    .divider()
    .element(approve)
    .build()?;

let message_id = sender
    .card_message(Recipient::Chat("oc_xxx".to_owned()), card)
    .send()
    .await?;
```

Use `OpenApiClient::update_message_card` for unconditional replacement of an inline sent card by `message_id`. For CardKit entity workflows, create the entity with `OpenApiClient::create_card_entity` and send its `CardId` with `card_reference_message` or `card_reference_reply`. Use `OpenApiClient::update_card_entity` only for full-card replacement; native streaming uses `OpenApiClient::update_card_element_content` for accumulated text and `OpenApiClient::update_card_settings` to close streaming mode. Every CardKit entity operation requires a strictly increasing sequence. See [lark-api.md](lark-api.md) for exact endpoint mappings and lifecycle constraints.

## High-Level Markdown Streaming

`MessageSender::markdown_stream_message` sends a new CardKit stream to a chat or user. `MessageSender::markdown_stream_reply` replies to an existing message and can opt into thread placement. Both return `MarkdownStreamBuilder`; `start` creates the CardKit entity, sends its reference, and returns a `MarkdownStream` that owns sequence progression and operation idempotency values.

```rust
use std::time::Duration;

let mut builder = sender
    .markdown_stream_reply(MessageId("om_xxx".to_owned()))
    .reply_in_thread(true)
    .initial_text("Thinking...")
    .streaming_summary("[Generating...]");

let mut stream = builder
    .start()
    .await?
    .throttle(Duration::from_millis(150));
stream.append("First chunk").await?;
stream.append(" and second chunk").await?;
stream.finish().await?;
```

`MarkdownStream::append` treats its input as a delta and immediately sends the accumulated full text. `set_content` accepts a complete snapshot instead. Empty chunks and identical snapshots are no-ops. `finish` supplies the configured empty fallback if no output was produced, disables streaming mode, and derives a compact final preview summary unless `final_summary` overrides it.

For token-oriented producers, consume the active stream with `MarkdownStream::throttle`. The resulting `ThrottledMarkdownStream` sends the first content immediately and coalesces later snapshots until the configured interval elapses. Content calls drive due updates; the wrapper does not create a background task or depend on a particular async runtime. If a producer can pause while content remains buffered, use `next_flush_in` with the application's timer and then call `flush`, or call `flush` directly to bypass the interval. `finish` also bypasses the interval so it can flush the latest tail before closing streaming mode. `content` returns the latest logical content, `flushed_content` returns the acknowledged content, and `has_buffered_content` distinguishes them.

The platform currently limits card operations on one entity to 10 updates per second. The throttle interval covers automatic content updates only; explicit `flush` and `finish` operations are not delayed. Applications that require a hard aggregate limit must schedule those operations too. The runnable example uses at least 150 milliseconds between automatic content updates to leave room for its final tail and settings operations, while the SDK accepts other intervals for caller-owned policies and deterministic tests. Content and finish validation enforce the 100,000-character streaming field limit and the separate 30 KiB whole-card limit before an update. When complete single-card output is already known, `MarkdownStreamBuilder::preflight_content` checks both limits before card creation or delivery.

Call `MarkdownStreamBuilder::start_continuing` when generated output may exceed one card. It returns `ContinuingMarkdownStream`, which keeps accepted source append-only, closes full cards, sends follow-up card-reference messages, and retains replayable ambiguous update, finish, or delivery operations for `retry_pending`. Card entity creation is non-idempotent: an unknown initial creation outcome prevents the builder from starting a stream, while an unknown follow-up creation outcome makes `is_recovery_blocked` return true. Retry then returns a validation error rather than risking a duplicate entity; `has_pending_operation` is false and `next_flush_in` returns `None`. Each page owns an independent CardKit sequence and idempotency lifecycle, while `pages` exposes every acknowledged `CardId` and `MessageId` in order.

```rust
let mut stream = sender
    .markdown_stream_message(Recipient::Chat("oc_xxx".to_owned()))
    .continuation_max_page_chars(30_000)
    .start_continuing()
    .await?
    .throttle(Duration::from_millis(150));

stream.append("First chunk").await?;
stream.append(" and a later chunk").await?;
stream.finish().await?;
```

Continuation preserves the original UTF-8 source exactly and prefers paragraph, line, and whitespace boundaries. Pagination is format-agnostic: it never separates a UTF-8 character, but it may split any Markdown construct, line-ending pair, or other source sequence across pages. Content is not rewritten, and every page renders independently according to the current Lark/Feishu client.

`start` does not retry CardKit entity creation because that endpoint has no idempotency key. If the creation response itself is lost, the resulting `CardId` cannot be recovered and creating again may allocate another unsent entity. After entity creation is acknowledged, the builder keeps the prepared `CardId`, target, delivery options, and message UUID until message delivery is acknowledged. If delivery has an ambiguous transport, non-success HTTP status, or response-decoding outcome, retain the same mutable builder and retry through the same entrypoint: call `start` for a single-card stream or `start_continuing` for a continuing stream. `prepared_card_id` reports whether creation reached that reusable prepared state.

For content and finish operations, internal transport retries reuse the same payload, sequence, and UUID. If every attempt ends with an ambiguous transport, non-success HTTP status, or response-decoding failure, `MarkdownStream` retains that exact operation and reports it through `has_pending_operation`. Call `retry_pending`, or repeat the same content operation, before changing content. `ThrottledMarkdownStream` replays retained content before sending any newer buffered snapshot; `flush` and `finish` preserve that ordering. API rejection, validation, and request-preparation failures are definitive and clear pending state without advancing acknowledged content or sequence. Dropping an unfinished stream cannot perform asynchronous cleanup; the platform closes streaming mode after its timeout, but callers should normally call `finish` explicitly.

To acknowledge a card action with immediate feedback:

```rust
use lark_channel::{CardActionResponse, CardActionToast, CardActionToastType};

let ack = CardActionResponse::new()
    .with_toast(CardActionToast::new(
        CardActionToastType::Success,
        "Action accepted",
    ))
    .to_websocket_ack()?;
```

If the card must be updated later, return the ACK first and enqueue the callback token. Work that runs after the ACK can then call:

```rust
openapi
    .update_message_card_with_callback_token(callback_token, &updated_card)
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

Creating or updating CardKit entities also requires the `cardkit:card:write` permission ("Create and update cards"). Inline message-card replacement uses the message update permissions documented by the official API and only supports messages sent within the previous 14 days.

Message-reading permissions are separate from send permissions. For example, reading group message history requires `im:message.group_msg`, and reading group members requires a chat member read permission such as `im:chat.members:read`. Those read-side APIs are not part of the current message scope.
