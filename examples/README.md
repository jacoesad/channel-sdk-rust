# Examples

Examples show how applications can call the SDK from ordinary Rust code.

## Token request

`tokens.rs` requests both app and tenant access tokens with the default reqwest transport.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
cargo run --example tokens
```

For local development, you can keep these variables in a git-ignored `.env` file and load it in your shell before running the example:

```bash
set -a
source .env
set +a
cargo run --example tokens
```

The example prints token lengths only. It does not print token values.

## Send text message

`send_text.rs` sends a text message to a chat or user with the default reqwest transport.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_CHAT_ID=oc_xxx
export LARK_TEXT="hello from lark-channel"
# Optional idempotency key:
export LARK_UUID=uuid_xxx
cargo run --example send_text
```

Use `LARK_OPEN_ID=ou_xxx` instead of `LARK_CHAT_ID` to send a direct message to a user by open id. If both are set, `LARK_CHAT_ID` takes priority. See [../docs/messages.md](../docs/messages.md) for recipient semantics.

`LARK_TEXT` is optional and defaults to a short greeting. `LARK_UUID` is optional and is sent as the OpenAPI idempotency key when set. The example prints the returned message id.

## Send with MessageSender

`message_sender.rs` sends a text message through the high-level `MessageSender`.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_CHAT_ID=oc_xxx
export LARK_TEXT="hello from lark-channel"
export LARK_MAX_ATTEMPTS=3
export LARK_UUID=uuid_xxx
cargo run --example message_sender
```

Use `LARK_OPEN_ID=ou_xxx` instead of `LARK_CHAT_ID` to send a direct message to a user by open id. If both are set, `LARK_CHAT_ID` takes priority.

`MessageSender` generates an idempotency key automatically and reuses it across transport-failure retries. Set `LARK_UUID` to provide a stable upstream key for process restarts or queue replays. `LARK_MAX_ATTEMPTS` is optional and defaults to `3`. The example prints the returned message id.

## Send Markdown rich text

`send_markdown.rs` sends Markdown as a Lark/Feishu `post` message through `MessageSender`. The Markdown is wrapped in the official native `md` element instead of being parsed by the SDK.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_CHAT_ID=oc_xxx
export LARK_MARKDOWN=$'## Build status\n\n- **Passed**\n- [Details](https://example.com)'
# Optional post title and idempotency key:
export LARK_TITLE="Agent update"
export LARK_UUID=uuid_xxx
cargo run --example send_markdown
```

Use `LARK_OPEN_ID=ou_xxx` instead of `LARK_CHAT_ID` to send a direct message. If both are set, `LARK_CHAT_ID` takes priority. `LARK_MARKDOWN` and `LARK_TITLE` are optional; the example prints the returned message id.

## Reply to a message

`reply_text.rs` replies to an existing message with the default reqwest transport.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_MESSAGE_ID=om_xxx
export LARK_TEXT="reply from lark-channel"
# Optional idempotency and thread placement:
export LARK_UUID=uuid_xxx
export LARK_REPLY_IN_THREAD=true
cargo run --example reply_text
```

`LARK_MESSAGE_ID` is the parent message id to reply to. When the parent message belongs to a thread or topic, Lark/Feishu places the reply under that conversation context. `LARK_TEXT` is optional and defaults to a short reply. `LARK_UUID` is optional and is sent as the OpenAPI idempotency key when set. `LARK_REPLY_IN_THREAD` is optional and accepts `true`/`false`. The example prints the returned message id.

## Send and update cards

`cards.rs` builds a CardKit 2.0 card with common typed components and sends it through `MessageSender`.

Enable the relevant IM send permission for ordinary card messages. Creating or updating a CardKit entity additionally requires the `cardkit:card:write` permission ("Create and update cards") in the Lark/Feishu developer console.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_CHAT_ID=oc_xxx
cargo run --example cards
```

Set `LARK_UPDATE_CARD=true` to replace the sent inline card by `message_id`. The builder emits `config.update_multi=true`, which the official message-card update API requires on both the original and updated card. Inline replacement is only available within 14 days after the message is sent.

Use a CardKit entity when later component-level or streaming updates need a stable `card_id`:

```bash
export LARK_CARD_ENTITY=true
export LARK_UPDATE_CARD=true
export LARK_CARD_SEQUENCE=1
# Optional CardKit update idempotency key:
export LARK_CARD_UPDATE_UUID=card-update-1
cargo run --example cards
```

CardKit entities can be sent once and remain valid for 14 days. Every operation on one entity must use a `sequence` greater than its previous CardKit operation; the SDK validates the documented integer range but the caller owns ordering across concurrent tasks or process restarts.

## Stream CardKit text

`card_streaming.rs` exercises the low-level CardKit streaming protocol. It creates and sends a streaming card entity, updates one Markdown or nested plain-text element with full accumulated content, then disables streaming mode and replaces the chat preview summary.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_CHAT_ID=oc_xxx
# Optional final content and element kind (`markdown` is the default):
export LARK_STREAM_TEXT="Streaming update completed."
export LARK_STREAM_ELEMENT=plain_text
cargo run --example card_streaming
```

`LARK_STREAM_ELEMENT` accepts `markdown` or `plain_text`. Markdown stores `element_id` on the top-level component, while the plain-text CardKit component is a `div` whose actual streaming target is the nested `plain_text`; the example assigns the identifier at the correct level for each form.

`LARK_STREAM_TEXT` must contain between 2 and 100,000 Unicode characters, and the resulting serialized card JSON must remain within the platform's separate 30 KiB (30,720 UTF-8 bytes) whole-card limit. The example validates the complete streaming and closed card states before any OpenAPI call, initializes the selected text element with its first character, then sends the complete text so the update is a prefix-preserving append and the client can render the native typewriter effect. It uses sequence `1` for the content update and `2` for the settings update. Production callers must coordinate a strictly increasing sequence across every operation on the same card. This example remains useful when an application needs direct control of the low-level protocol.

## Stream Markdown with MessageSender

`markdown_stream.rs` uses the high-level `MessageSender` lifecycle. It creates and sends Markdown CardKit entities, accumulates chunk updates, rolls oversized output onto follow-up messages, owns sequence and idempotency values, and closes streaming mode when output finishes.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx

# Send a new stream to a chat (LARK_OPEN_ID is also supported):
export LARK_CHAT_ID=oc_xxx

# Or reply to an existing message instead of setting a recipient:
# export LARK_MESSAGE_ID=om_xxx
# export LARK_REPLY_IN_THREAD=true

export LARK_STREAM_TEXT=$'## Result\n\nStreaming output from lark-channel.'
cargo run --example markdown_stream
```

`LARK_STREAM_TEXT` must be non-empty. `LARK_STREAM_CHUNK_CHARS` controls the Unicode character count per generated token batch and defaults to `12`. `LARK_STREAM_CHUNK_DELAY_MS` simulates the delay between batches and defaults to `25`. `LARK_STREAM_INTERVAL_MS` configures the automatic content-update throttle, defaults to `150`, and must be at least `150` so the example leaves headroom under the platform's per-card operation guidance for its final tail and settings updates. `LARK_CONTINUATION_MAX_PAGE_CHARS` sets the soft per-page Unicode character limit and defaults to `30000`; the SDK may split earlier to satisfy the serialized-card limit. `LARK_UUID` controls the first message or reply idempotency key, and `LARK_MAX_ATTEMPTS` controls transport attempts.

Target variables use this precedence: `LARK_MESSAGE_ID`, then `LARK_CHAT_ID`, then `LARK_OPEN_ID`. Unset stale variables when changing between reply, chat, and direct-user tests.

The example starts a `ContinuingMarkdownStream` and applies its throttle interval. The first generated content is sent immediately, while later batches arriving inside `LARK_STREAM_INTERVAL_MS` are coalesced into one complete snapshot. Content calls drive due updates; the controller creates no background timer. Applications whose producer can pause should schedule `flush` using `next_flush_in`, and `finish` always flushes the final buffered tail. Explicit `flush` and `finish` calls bypass the content interval, so applications that require a strict aggregate operation budget must pace those calls too. The example configures one internal attempt and retries replayable ambiguous transport, HTTP-status, and response-decoding outcomes itself, preserving pending sequence and UUID state.

Each continuation page is checked against both the 100,000-character element limit and 30 KiB whole-card limit before it is sent. The splitter prefers paragraph, line, and whitespace boundaries, never separates a UTF-8 character, and preserves the complete source text. It is intentionally format-agnostic: Markdown constructs and line-ending pairs may span pages, no syntax is rewritten, and each page renders independently.

## Download a message resource

`download_resource.rs` downloads an image or file resource already attached to a received message. It keeps the bytes in memory and prints only their length and response metadata.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_MESSAGE_ID=om_xxx
export LARK_RESOURCE_KEY=img_xxx
export LARK_RESOURCE_TYPE=image
cargo run --example download_resource
```

`LARK_RESOURCE_TYPE` accepts `image` or `file`. Use `file` for ordinary files, audio, and video. The bot must belong to the message conversation, and the resource must not exceed the platform's 100 MB limit. The example does not write the response to disk.

## Upload a resource

`upload_resource.rs` reads one caller-selected local file into memory, uploads it, and prints the returned resource key:

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_UPLOAD_PATH=/absolute/path/to/image.png
export LARK_UPLOAD_TYPE=image
cargo run --example upload_resource
```

`LARK_UPLOAD_TYPE` accepts `image`, `file`, `opus`, or `mp4`. OPUS audio and MP4 video additionally require a positive `LARK_UPLOAD_DURATION_MS` and a matching `.opus` or `.mp4` filename. The example does not transcode media. Images use the official `message` image type; generic files use `stream`. Images are limited to 10 MB and files to 30 MB.

The example performs the local file read explicitly. `MediaUploader` itself accepts in-memory bytes and does not choose paths, fetch URLs, infer duration, or retry an upload.

## WebSocket endpoint and connection

`ws_connect.rs` requests the long-connection WebSocket endpoint. By default it prints redacted endpoint metadata only. Set `LARK_WS_CONNECT=1` to open the WebSocket connection and close it immediately. Add `LARK_WS_RECEIVE_ONCE=1` to wait for one event through `EventConsumer`, print parsed event metadata including each resource descriptor's OpenAPI message ID and resource key, send an ACK, and close. Those values can be passed to `download_resource.rs`.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
cargo run --example ws_connect --features websocket

# Optional real connection smoke test:
export LARK_WS_CONNECT=1
cargo run --example ws_connect --features websocket

# Optional receive-and-ack smoke test:
export LARK_WS_CONNECT=1
export LARK_WS_RECEIVE_ONCE=1
cargo run --example ws_connect --features websocket
```

The real connection modes consume one long-connection slot while connected. Lark/Feishu currently limits each app to 50 long connections.

The endpoint URL can include transient connection material, so the example does not print the full query string.

The library also exposes low-level event helpers behind the `websocket` feature. Use `WebSocketConnection::next_event` to receive an event data frame and `WebSocketConnection::ack_event` to acknowledge it after your handler finishes. `next_event` returns the event payload separately from the lightweight ACK frame metadata, avoiding a second copy of large event payloads.

For application code, prefer `EventConsumer` when you want a single-connection receive/parse/handler/ACK loop. `handle_next_event` adds a `biz_rt` ACK header when the handler returns an ACK without one. If parsing fails after a complete event is received, `EventConsumer` attempts to send an internal-server-error ACK before returning. Handler errors are also ACKed as internal-server-error before the original handler error is returned. High-level event handling reassembles Lark/Feishu application-level split packets whose `sum > 1` before invoking the handler; `WebSocketConnection::next_event` remains the lower-level raw-frame API.

`ws_event_loop.rs` runs the higher-level reconnecting `EventLoop`. It requests a fresh WebSocket endpoint for each connection attempt, reconnects after clean closes and transport errors, sends application-level heartbeat pings at the endpoint-provided `PingInterval` with a 120-second fallback while waiting for events, prints parsed event metadata, and ACKs handled events. Card actions receive a success Toast through `CardActionResponse`.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=app_secret

# Runs until the reconnect limit is reached or the process is stopped.
LARK_WS_MAX_RECONNECTS=3 LARK_WS_RECONNECT_DELAY_MS=1000 \
cargo run --example ws_event_loop --features websocket
```

The example uses local reconnect defaults (`LARK_WS_MAX_RECONNECTS=3`, `LARK_WS_RECONNECT_DELAY_MS=1000`) so local smoke tests terminate predictably. Set `LARK_WS_USE_SERVER_RECONNECT_CONFIG=true` to follow endpoint-provided reconnect policy instead. Set `LARK_WS_HEARTBEAT_TIMEOUT_MS` to enable an optional liveness watchdog after application-level heartbeat pings sent while waiting for events.

The loop responds to WebSocket ping frames through the underlying connection and sends the official application-level heartbeat ping while waiting for events. Handler futures are awaited without driving connection heartbeats; keep handlers short or spawn long-running work outside the loop. Heartbeat send failures and optional heartbeat liveness timeouts are treated as reconnectable transport errors. `EventLoop` reassembles split packets before invoking handlers and keeps receive, dispatch, and protocol writes as separate internal runtime responsibilities; independently driven heartbeat and writer tasks remain follow-up work.

For delayed card updates, return the callback ACK first and hand the callback token to work that runs afterward. Call `OpenApiClient::update_message_card_with_callback_token` from that post-ACK work. Tokens are valid for 30 minutes and can be used at most twice; calling the update before or concurrently with the ACK is not supported by the platform lifecycle.

## Minimal echo bot

`echo_bot.rs` combines `EventLoop` and `MessageSender` into a minimal bot. It listens for message events, replies to text messages with `echo: <text>`, and ACKs skipped or handled events.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=app_secret

# Optional for group chats: reply only when this bot is mentioned.
export LARK_BOT_OPEN_ID=ou_xxx

cargo run --example echo_bot --features websocket
```

Private chat text messages are echoed by default. Group messages are echoed only when `LARK_BOT_OPEN_ID` is set and the message mentions that bot. Set `LARK_ECHO_ALL_GROUP_MESSAGES=true` to echo all group text messages. `LARK_ECHO_PREFIX`, `LARK_ECHO_REPLY_IN_THREAD`, `LARK_MAX_ATTEMPTS`, and the same `LARK_WS_*` reconnect options used by `ws_event_loop.rs` are optional.
