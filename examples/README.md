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

## WebSocket endpoint and connection

`ws_connect.rs` requests the long-connection WebSocket endpoint. By default it prints redacted endpoint metadata only. Set `LARK_WS_CONNECT=1` to open the WebSocket connection and close it immediately.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
cargo run --example ws_connect --features websocket

# Optional real connection smoke test:
export LARK_WS_CONNECT=1
cargo run --example ws_connect --features websocket
```

The real connection mode consumes one long-connection slot while it is connected. Lark/Feishu currently limits each app to 50 long connections.

The endpoint URL can include transient connection material, so the example does not print the full query string.
