# channel-sdk-rust

Lark/Feishu Channel SDK for Rust.

This repository is an early community scaffold for a Rust SDK that mirrors the role of the official Channel SDK family:

- `channel-sdk-go`
- `channel-sdk-java`
- `channel-sdk-python`
- `channel-sdk-node`

The first target is to support agent/bot bridges such as `lark-coding-agent-bridge-rs`: inbound events, normalized messages, streaming replies, media downloads, and interactive card callbacks.

## Status

Experimental. The crate currently contains the public module skeleton and shared data types. Network transport and OpenAPI implementations are intentionally left for the next milestone.

## Planned Modules

- `config`: app id/secret, Feishu/Lark domain selection, SDK source metadata
- `event`: normalized inbound events
- `message`: normalized messages and outbound content
- `card`: interactive card primitives
- `media`: resource descriptors and download/upload helpers
- `client`: async client trait for transport implementations

## Suggested Crate Name

The package/repository is named `channel-sdk-rust` to match LarkSuite's official SDK naming style. The Rust library target is `lark_channel`, so users can import it as:

```rust
use lark_channel::{ChannelConfig, Domain};
```

## Roadmap

1. Implement app access-token management.
2. Add OpenAPI HTTP client primitives.
3. Add WebSocket event connection and event acknowledgement.
4. Port message normalization semantics from `channel-sdk-node`.
5. Add card create/update helpers.
6. Add media download/upload helpers.
7. Build a minimal echo bot example.
