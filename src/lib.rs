//! Lark/Feishu Channel SDK for Rust.
//!
//! This crate is an early scaffold for a Rust equivalent of the Lark Channel SDK:
//! reliable inbound events, normalized messages, streaming replies, media helpers,
//! and interactive card callbacks.

pub mod card;
pub mod client;
pub mod config;
pub mod error;
pub mod event;
pub mod lark_openapi;
pub mod media;
pub mod message;

pub use client::{ChannelClient, ChannelClientExt};
pub use config::{ChannelConfig, Domain};
pub use error::{Error, Result};
pub use event::{
    CardActionContext, CardActionEvent, CardActionOperator, CardActionPayload, ChannelEvent,
    EventContext, parse_lark_event_payload,
};
#[cfg(feature = "websocket")]
pub use event::{
    EventConnection, EventConnectionItem, EventConsumer, EventLoop, EventLoopExit,
    EventLoopOptions, EventPacketReassemblyOptions, EventReconnectLimit, EventStreamConnector,
    OpenApiWebSocketEventConnector, ReceivedEvent, WebSocketEndpointConnector,
};
pub use media::{ResourceDescriptor, ResourceType};
pub use message::{
    MessageBuilder, MessageChatType, MessageContent, MessageId, MessageMention,
    MessageReplyBuilder, MessageSender, MessageSenderInfo, MessageSenderOptions, MessageSenderType,
    NormalizedMessage, Recipient,
};
