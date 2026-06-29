use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use url::Url;

#[cfg(feature = "reqwest-transport")]
use crate::Error;
use crate::Result;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: Url,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

impl HttpRequest {
    pub fn post_json(url: Url, body: Value) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        Self {
            method: HttpMethod::Post,
            url,
            headers,
            body,
        }
    }

    pub fn with_bearer_auth(mut self, token: impl Into<String>) -> Self {
        self.headers.insert(
            "authorization".to_owned(),
            format!("Bearer {}", token.into()),
        );
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Value,
}

impl HttpResponse {
    pub fn json(status: u16, body: Value) -> Self {
        Self { status, body }
    }
}

pub trait OpenApiTransport: Clone + Send + Sync + 'static {
    fn send_json(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse>>;
}

#[cfg(feature = "reqwest-transport")]
#[derive(Debug, Clone)]
pub struct ReqwestOpenApiTransport {
    client: reqwest::Client,
}

#[cfg(feature = "reqwest-transport")]
impl ReqwestOpenApiTransport {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[cfg(feature = "reqwest-transport")]
impl Default for ReqwestOpenApiTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "reqwest-transport")]
impl OpenApiTransport for ReqwestOpenApiTransport {
    fn send_json(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse>> {
        let client = self.client.clone();

        Box::pin(async move {
            let HttpRequest {
                method,
                url,
                headers,
                body,
            } = request;

            let mut builder = client.request(method.into(), url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            if !body.is_null() {
                builder = builder.json(&body);
            }

            let response = builder
                .send()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;
            let status = response.status().as_u16();
            let body = response
                .json::<Value>()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;

            Ok(HttpResponse { status, body })
        })
    }
}

#[cfg(feature = "reqwest-transport")]
impl From<HttpMethod> for reqwest::Method {
    fn from(method: HttpMethod) -> Self {
        match method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
        }
    }
}
