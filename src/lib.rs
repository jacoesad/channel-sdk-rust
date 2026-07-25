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
mod validation;

pub use card::{
    Card, CardBuilder, CardButtonStyle, CardElement, CardElementContent, CardId, CardSettings,
    CardStreamingConfig, CardStreamingPlatformValues, CardStreamingPrintStrategy,
    MAX_CARD_ELEMENT_CONTENT_CHARS, MAX_CARD_JSON_BYTES,
};
pub use client::{ChannelClient, ChannelClientExt};
pub use config::{ChannelConfig, Domain};
pub use error::{Error, Result};
pub use event::{
    CardActionContext, CardActionEvent, CardActionOperator, CardActionPayload, CardActionResponse,
    CardActionToast, CardActionToastType, ChannelEvent, EventContext, parse_lark_event_payload,
};
#[cfg(feature = "websocket")]
pub use event::{
    EventConnection, EventConnectionItem, EventConsumer, EventLoop, EventLoopExit,
    EventLoopOptions, EventPacketReassemblyOptions, EventReconnectLimit, EventStreamConnector,
    OpenApiWebSocketEventConnector, ReceivedEvent, WebSocketEndpointConnector,
};
pub use media::{DownloadedResource, MediaDownloader, ResourceDescriptor, ResourceType};
pub use message::{
    ContinuingMarkdownStream, MarkdownStream, MarkdownStreamBuilder, MarkdownStreamPage,
    MessageBuilder, MessageChatType, MessageContent, MessageId, MessageMention,
    MessageReplyBuilder, MessageSender, MessageSenderInfo, MessageSenderOptions, MessageSenderType,
    NormalizedMessage, PostContent, PostContentBuilder, PostDocument, PostElement, PostStyle,
    Recipient, ThrottledMarkdownStream,
};
