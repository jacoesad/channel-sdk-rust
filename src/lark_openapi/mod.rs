//! Minimal Feishu/Lark OpenAPI subset used by `lark-channel`.
//!
//! This module contains the OpenAPI client foundation used by this crate:
//! authentication, transport abstraction, response parsing, and the OpenAPI
//! resources needed by Channel workflows.
//!
//! This is not a complete OpenAPI SDK. Low-level OpenAPI types are exposed
//! through this dedicated namespace so they remain distinguishable from the
//! Channel SDK API and replaceable by a future external `lark-openapi` crate
//! or an official Rust OpenAPI SDK adapter.

mod auth;
mod message;
mod response;
mod transport;
mod ws;

use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::{ChannelConfig, Result};

pub use auth::{AppAccessTokenResponse, TenantAccessTokenResponse};
pub use message::{MessageCreateOptions, MessageReplyOptions};
#[cfg(feature = "reqwest-transport")]
pub use transport::ReqwestOpenApiTransport;
pub use transport::{BoxFuture, HttpMethod, HttpRequest, HttpResponse, OpenApiTransport};
#[cfg(feature = "websocket")]
pub use ws::{TokioTungsteniteWebSocketTransport, WebSocketConnection};
pub use ws::{
    WebSocketClientConfig, WebSocketEndpoint, WebSocketFrame, WebSocketFrameMethod,
    WebSocketHeader, WebSocketMessageType,
};

use auth::AccessTokenCache;
use response::parse_openapi_response;

#[derive(Debug, Clone)]
pub struct OpenApiClient<T> {
    config: ChannelConfig,
    transport: T,
    app_access_token_cache: Arc<AccessTokenCache>,
    tenant_access_token_cache: Arc<AccessTokenCache>,
}

impl<T> OpenApiClient<T>
where
    T: OpenApiTransport,
{
    pub fn new(config: ChannelConfig, transport: T) -> Self {
        Self {
            config,
            transport,
            app_access_token_cache: Arc::new(AccessTokenCache::default()),
            tenant_access_token_cache: Arc::new(AccessTokenCache::default()),
        }
    }

    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    pub async fn post_openapi_json<B, R>(&self, path: &str, body: &B) -> Result<R>
    where
        B: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let request = self.post_json_request(path, body)?;
        let response = self.transport.send_json(request).await?;
        parse_openapi_response(response)
    }

    pub async fn post_tenant_json<B, R>(&self, path: &str, body: &B) -> Result<R>
    where
        B: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let token = self.tenant_access_token().await?;
        let request = self.post_json_request(path, body)?.with_bearer_auth(token);
        let response = self.transport.send_json(request).await?;
        parse_openapi_response(response)
    }

    fn post_json_request<B>(&self, path: &str, body: &B) -> Result<HttpRequest>
    where
        B: Serialize + ?Sized,
    {
        let url = self.config.base_url().join(path)?;
        let body = serde_json::to_value(body)?;
        Ok(HttpRequest::post_json(url, body))
    }
}

#[cfg(test)]
mod tests;
