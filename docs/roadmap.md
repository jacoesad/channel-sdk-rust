# Roadmap

This document tracks the planned direction for `lark-channel`, a community Rust implementation aligned with the Lark/Feishu Channel SDK family.

The near-term goal is not to mirror the full OpenAPI SDK. The first useful target is a Channel SDK foundation for agent and bot bridge workloads: inbound events, normalized messages, outbound replies, card updates, and media helpers.

## Current Status

Milestone 0 established the repository and public crate shape:

- Rust crate metadata and library target `lark_channel`
- Public modules for config, client, event, message, card, media, and errors
- Shared data types for normalized messages and channel events
- A `ChannelClient` trait for future transport implementations
- CI for formatting, clippy, and tests

The project is now moving through Milestone 1 with an early transport-agnostic OpenAPI module, app and tenant access-token handling, and a default reqwest transport. It still does not implement WebSocket handling, outbound messaging, message normalization, card helpers, or media transfer yet.

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

- Send text messages to chats and users
- Reply to messages and threads
- Convert simple markdown/text into Feishu/Lark message content
- Basic retry and idempotency support
- Example for sending a text message

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
