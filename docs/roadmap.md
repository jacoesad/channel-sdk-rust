# Roadmap

This document tracks the planned direction for `lark-channel`, a community Rust implementation aligned with the Lark/Feishu Channel SDK family.

The near-term goal is not to mirror the full OpenAPI SDK. The first useful target is a Channel SDK foundation for agent and bot bridge workloads: inbound events, normalized messages, outbound replies, card updates, and media helpers.

## Current Status

Milestone 0 established the repository and public crate shape:

- Rust crate metadata and library target `lark_channel`
- Public modules for config, client, event, message, card, media, lark_openapi, and errors
- Shared data types for normalized messages and channel events
- A `ChannelClient` trait for future transport implementations
- CI for formatting, clippy, and tests

The project has completed Milestone 2 with outbound text messaging, replies, idempotency options, and the first high-level message sender. Milestone 3 has started with WebSocket endpoint discovery, a raw connection foundation, event frame parsing, explicit event acknowledgement, inbound message parsing, card action callback parsing, a single-connection event consumer, a reconnecting event loop, timer-driven application heartbeat, split-packet event reassembly, a minimal echo bot example, and the first internal runtime boundaries for receive/reassembly, dispatch, and protocol writes. It still does not implement full message normalization, card helpers, or media transfer yet.

## Architecture Boundary

`lark-channel` is a Channel SDK, not a full Lark/Feishu OpenAPI SDK. The `lark_openapi` module is a small internal OpenAPI subset for Channel workflows: authentication, transport abstraction, response parsing, and the OpenAPI resources needed by current milestones.

Feishu and Lark share the same OpenAPI-shaped request/response definitions for the resources used here. The selected environment is a `ChannelConfig` concern: `Domain::Feishu` is the default, and `Domain::Lark` switches the base URL to the Lark OpenAPI domain.

Keep `OpenApiClient` low-level and explicit. It should perform one OpenAPI request at a time and expose caller-provided request options. Managed behavior such as retry policy, fallback handling, automatic idempotency-key generation, idempotency reuse across retries, content conversion, and event/message normalization belongs in higher-level Channel modules such as `MessageSender`.

The `lark_openapi` module should remain replaceable: it may later be extracted into a standalone `lark-openapi` crate or swapped for an official Rust OpenAPI SDK adapter without rewriting the higher-level Channel workflow.

Low-level OpenAPI types should stay namespaced under `lark_channel::lark_openapi`. The crate root is reserved for the Channel SDK entry points and shared domain types so callers can tell which layer they are using.

The currently implemented OpenAPI surface is tracked in [lark-api.md](lark-api.md).

## WebSocket Runtime Direction

The current WebSocket event loop is intentionally conservative. It owns one connection, sends application-level heartbeat pings only while waiting for the next event, then runs the user handler and sends the ACK. Internally, the receive/reassembly path, handler dispatch/ACK policy, and protocol write facade are separated so the responsibilities are clear. They still run over one mutable connection, so the loop does not drive connection heartbeats while a handler future is running.

The target runtime model should use a connection runtime that owns the WebSocket lifecycle. A receive loop should read data and control frames and publish parsed events. A heartbeat loop should schedule application-level heartbeat pings from the latest endpoint-provided client configuration. A reconnect controller should own connection replacement and restart the runtime after reconnectable failures. A serialized writer path should be the only component allowed to write protocol frames such as ACK, heartbeat, and close frames. User handlers should receive parsed events and return ACK policy without owning the connection. That model would allow long-running handlers and connection heartbeats to coexist without competing for the same mutable connection.

## Version Policy

During the early `0.x` series, releases generally correspond to completed roadmap milestones rather than every merged feature PR:

- `v0.1.0`: Milestone 1, OpenAPI foundation
- `v0.2.0`: Milestone 2, outbound messaging
- `v0.3.0`: Milestone 3, events and WebSocket
- `v0.4.0`: Milestone 4, message normalization
- `v0.5.0`: Milestone 5, rich content, cards, and streaming replies
- `v0.6.0`: Milestone 6, media helpers

Patch versions such as `v0.2.1` are reserved for bug fixes or small follow-up improvements within a completed milestone. Release PRs such as `release/v0.4.0` should stay separate and contain only release metadata, changelog/readme version updates, and publish dry-run fixes.

Milestone bullets are intended as default PR boundaries. A PR should usually complete one bullet. Closely related small bullets may be grouped into one PR, and unusually large bullets may be split into several focused PRs. Completed bullets may include the PRs that delivered them.

## Milestone 0: Project Foundation

- MIT license and repository initialization (#1)
- Rust crate scaffold and public module layout (#1)
- Initial shared types for config, events, messages, cards, media, and errors (#1)
- Initial `ChannelClient` trait shape (#1)
- README with project status and scope (#1)
- Roadmap for staged development (#1)
- CI for formatting, clippy, and tests (#1)

Milestone 0 is complete when the scaffold is reviewable and the repository has enough structure for feature work to proceed through focused follow-up PRs.

## Milestone 1: OpenAPI Foundation

- App and tenant access token requests with in-memory cache (#4)
- Feishu/Lark domain selection through safe built-in domains (#4)
- Typed API error parsing (#4)
- Transport-agnostic OpenAPI client abstraction (#4)
- Tests for token refresh and API error handling (#4)

## Milestone 2: Outbound Messaging

- OpenAPI-level helpers for sending text messages to chats and users (#6)
- OpenAPI-level helpers for replying to messages and threads (#7)
- Idempotency options for OpenAPI send and reply calls (#8)
- Managed `MessageSender` with basic retry and automatic idempotency reuse (#10)
- Runnable examples for sending and replying to messages (#6, #7, #10)

## Milestone 3: Events and WebSocket

- WebSocket connection lifecycle (#12)
- Event acknowledgement (#13)
- Message receive events (#14)
- Single-connection event consumer for receive, parse, handler, and ACK (#15)
- Basic reconnecting event loop for clean closes and transport errors (#16)
- Card action events (#17)
- Timer-driven heartbeat and reconnect refinements
- Split-packet reassembly for large long-connection events and callbacks with `sum > 1`
- Minimal echo bot example (#20)
- WebSocket runtime refactor with internal receive/reassembly, dispatch/ACK, and protocol writer boundaries while heartbeat and reconnect remain loop-orchestrated

## Milestone 4: Message Normalization

- Normalize text and post messages
- Normalize mentions
- Preserve raw event payloads for unsupported message types
- Add converters for common media/resource messages
- Align semantics with `channel-sdk-node` where practical

## Milestone 5: Rich Content, Cards, and Streaming Replies

- Simple Markdown/text conversion into Feishu/Lark rich message content
- Structured mention and link helpers where supported by Lark/Feishu message formats
- Card creation and update helpers
- Card callback response and update helpers
- Markdown streaming reply helper
- Update throttling for long-running agent output
- Continuation behavior for long messages

## Milestone 6: Media Helpers

- Download message resources
- Upload images/files where supported
- Resource descriptors with filenames and MIME hints
- Path and SSRF safety checks for URL-based media

## Later Scope

- Comment/document surfaces
- Registration helpers
- Topic-group specific behavior
- Multi-platform service helpers
- Persistent token/cache storage

## Non-Goals

- Reimplementing the full Lark/Feishu OpenAPI SDK
- Hiding raw OpenAPI escape hatches from advanced users
- Guaranteeing API parity with Node/Python before the Rust API has stabilized
