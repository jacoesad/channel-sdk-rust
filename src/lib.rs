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
pub mod media;
pub mod message;
pub mod openapi;

pub use client::{ChannelClient, ChannelClientExt};
pub use config::{ChannelConfig, Domain};
pub use error::{Error, Result};
pub use event::{ChannelEvent, EventContext};
pub use message::{MessageContent, NormalizedMessage};
#[cfg(feature = "reqwest-transport")]
pub use openapi::ReqwestOpenApiTransport;
pub use openapi::{
    AppAccessTokenResponse, HttpMethod, HttpRequest, HttpResponse, OpenApiClient, OpenApiTransport,
    TenantAccessTokenResponse,
};
