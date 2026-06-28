# lark-channel

Lark/Feishu Channel SDK for Rust.

This repository is an early community scaffold for a Rust SDK that mirrors the role of the official Channel SDK family:

- `channel-sdk-go`
- `channel-sdk-java`
- `channel-sdk-python`
- `channel-sdk-node`

The first target is to support agent/bot bridges such as `lark-coding-agent-bridge-rs`: inbound events, normalized messages, streaming replies, media downloads, and interactive card callbacks.

## Status

Experimental. The crate currently contains the public module skeleton, shared data types, an OpenAPI foundation for app and tenant access-token management, and minimal outbound text messaging and replies.

## Planned Modules

- `config`: app id/secret, Feishu/Lark domain selection, SDK source metadata
- `event`: normalized inbound events
- `message`: normalized messages and outbound content
- `card`: interactive card primitives
- `media`: resource descriptors and download/upload helpers
- `client`: async client trait for transport implementations
- `openapi`: access-token cache, low-level OpenAPI response handling, and default reqwest transport

## Crate Name

The repository is named `channel-sdk-rust` to match LarkSuite's official SDK naming style. The published Rust crate is `lark-channel`, and the Rust library target is `lark_channel`, so users can import it as:

```toml
lark-channel = "0.1"
```

```rust
use lark_channel::{ChannelConfig, Domain};
```

## Examples

Examples live in [examples](examples).

The token example verifies the current OpenAPI foundation by requesting app and tenant access tokens:

```bash
export LARK_APP_ID=cli_xxx
export LARK_APP_SECRET=xxx
cargo run --example tokens
```

The example reads credentials from environment variables. Applications using this SDK may load those values from their own configuration system, secret manager, or local `.env` workflow before constructing `ChannelConfig`.

Minimal text messages can be sent through the OpenAPI client:

```rust
use lark_channel::{ChannelConfig, OpenApiClient, Recipient, ReqwestOpenApiTransport};

// Inside async application code:
let app_id = "cli_xxx";
let app_secret = "app_secret";
let chat_id = "oc_xxx";

let config = ChannelConfig::new(app_id, app_secret);
let client = OpenApiClient::new(config, ReqwestOpenApiTransport::new());
let message_id = client
    .send_text_message(Recipient::Chat(chat_id.to_owned()), "hello")
    .await?;
```

`app_id` and `app_secret` come from the Lark/Feishu developer console. See [docs/outbound-messages.md](docs/outbound-messages.md) for outbound message semantics and [examples/README.md](examples/README.md) for runnable example configuration.

## Roadmap

See [docs/roadmap.md](docs/roadmap.md) for the development plan.

The next milestones are:

1. App access-token management and OpenAPI HTTP primitives.
2. Outbound text messaging and reply helpers.
3. WebSocket event connection and event acknowledgement.
4. Message normalization aligned with `channel-sdk-node` where practical.
5. Card, streaming reply, and media helpers.
