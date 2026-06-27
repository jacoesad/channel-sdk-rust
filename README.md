# channel-sdk-rust

Lark/Feishu Channel SDK for Rust.

This repository is an early community scaffold for a Rust SDK that mirrors the role of the official Channel SDK family:

- `channel-sdk-go`
- `channel-sdk-java`
- `channel-sdk-python`
- `channel-sdk-node`

The first target is to support agent/bot bridges such as `lark-coding-agent-bridge-rs`: inbound events, normalized messages, streaming replies, media downloads, and interactive card callbacks.

## Status

Experimental. The crate currently contains the public module skeleton, shared data types, and an early transport-agnostic OpenAPI foundation for app access-token management.

## Planned Modules

- `config`: app id/secret, Feishu/Lark domain selection, SDK source metadata
- `event`: normalized inbound events
- `message`: normalized messages and outbound content
- `card`: interactive card primitives
- `media`: resource descriptors and download/upload helpers
- `client`: async client trait for transport implementations
- `openapi`: app access-token cache and low-level OpenAPI response handling

## Suggested Crate Name

The package/repository is named `channel-sdk-rust` to match LarkSuite's official SDK naming style. The Rust library target is `lark_channel`, so users can import it as:

```rust
use lark_channel::{ChannelConfig, Domain};
```

## Roadmap

See [docs/roadmap.md](docs/roadmap.md) for the development plan.

The next milestones are:

1. App access-token management and OpenAPI HTTP primitives.
2. Outbound text messaging and reply helpers.
3. WebSocket event connection and event acknowledgement.
4. Message normalization aligned with `channel-sdk-node` where practical.
5. Card, streaming reply, and media helpers.
