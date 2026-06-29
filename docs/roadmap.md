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

The project has completed the initial Milestone 1 OpenAPI token foundation and is moving through Milestone 2 with minimal outbound text messaging and replies. It still does not implement WebSocket handling, full message normalization, card helpers, or media transfer yet.

## Architecture Boundary

`lark-channel` is a Channel SDK, not a full Lark/Feishu OpenAPI SDK. The `lark_openapi` module is a small internal OpenAPI subset for Channel workflows: authentication, transport abstraction, response parsing, and the OpenAPI resources needed by current milestones.

Feishu and Lark share the same OpenAPI-shaped request/response definitions for the resources used here. The selected environment is a `ChannelConfig` concern: `Domain::Feishu` is the default, and `Domain::Lark` switches the base URL to the Lark OpenAPI domain.

Keep `OpenApiClient` low-level and explicit. It should perform one OpenAPI request at a time and expose caller-provided request options. Managed behavior such as retry policy, fallback handling, automatic idempotency-key generation, idempotency reuse across retries, content conversion, and event/message normalization belongs in higher-level Channel modules such as the planned `MessageSender`.

The `lark_openapi` module should remain replaceable: it may later be extracted into a standalone `lark-openapi` crate or swapped for an official Rust OpenAPI SDK adapter without rewriting the higher-level Channel workflow.

Low-level OpenAPI types should stay namespaced under `lark_channel::lark_openapi`. The crate root is reserved for the Channel SDK entry points and shared domain types so callers can tell which layer they are using.

## Version Policy

During the early `0.x` series, releases generally correspond to completed roadmap milestones rather than every merged feature PR:

- `v0.1.0`: Milestone 1, OpenAPI foundation
- `v0.2.0`: Milestone 2, outbound messaging
- `v0.3.0`: Milestone 3, events and WebSocket
- `v0.4.0`: Milestone 4, message normalization
- `v0.5.0`: Milestone 5, cards and streaming replies
- `v0.6.0`: Milestone 6, media helpers

Patch versions such as `v0.2.1` are reserved for bug fixes or small follow-up improvements within a completed milestone.

## Milestone 0: Project Foundation

- MIT license and repository initialization
- Rust crate scaffold and public module layout
- Initial shared types for config, events, messages, cards, media, and errors
- Initial `ChannelClient` trait shape
- README with project status and scope
- Roadmap for staged development
- CI for formatting, clippy, and tests

Milestone 0 is complete when the scaffold is reviewable and the repository has enough structure for feature work to proceed through focused follow-up PRs.

## Milestone 1: OpenAPI Foundation

- App and tenant access token requests with in-memory cache
- Feishu/Lark domain selection through safe built-in domains
- Typed API error parsing
- Transport-agnostic OpenAPI client abstraction
- Tests for token refresh and API error handling

## Milestone 2: Outbound Messaging

- OpenAPI-level helpers for sending text messages to chats and users
- OpenAPI-level helpers for replying to messages and threads
- Idempotency options for OpenAPI send and reply calls
- Managed `MessageSender` with basic retry and automatic idempotency reuse
- Simple markdown/text conversion into Feishu/Lark message content
- Runnable examples for sending and replying to messages

## Milestone 3: Events and WebSocket

- WebSocket connection lifecycle
- Event acknowledgement
- Message receive events
- Card action events
- Reconnect and keepalive behavior
- Minimal echo bot example

## Milestone 4: Message Normalization

- Normalize text and post messages
- Normalize mentions
- Preserve raw event payloads for unsupported message types
- Add converters for common media/resource messages
- Align semantics with `channel-sdk-node` where practical

## Milestone 5: Cards and Streaming Replies

- Card creation and update helpers
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
