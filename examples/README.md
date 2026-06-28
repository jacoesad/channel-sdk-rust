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
cargo run --example send_text
```

Use `LARK_OPEN_ID=ou_xxx` instead of `LARK_CHAT_ID` to send a direct message to a user by open id. If both are set, `LARK_CHAT_ID` takes priority. See [../docs/outbound-messages.md](../docs/outbound-messages.md) for recipient semantics.

`LARK_TEXT` is optional and defaults to a short greeting. The example prints the returned message id.

## Reply to a message

`reply_text.rs` replies to an existing message with the default reqwest transport.

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
export LARK_MESSAGE_ID=om_xxx
export LARK_TEXT="reply from lark-channel"
cargo run --example reply_text
```

`LARK_MESSAGE_ID` is the parent message id to reply to. When the parent message belongs to a thread or topic, Lark/Feishu places the reply under that conversation context. `LARK_TEXT` is optional and defaults to a short reply. The example prints the returned message id.
