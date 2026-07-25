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
    /// Creates a request without a body or implied content type.
    pub fn empty(method: HttpMethod, url: Url) -> Self {
        Self {
            method,
            url,
            headers: BTreeMap::new(),
            body: Value::Null,
        }
    }

    /// Creates a JSON request with the supplied HTTP method.
    pub fn json(method: HttpMethod, url: Url, body: Value) -> Self {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        Self {
            method,
            url,
            headers,
            body,
        }
    }

    pub fn post_json(url: Url, body: Value) -> Self {
        Self::json(HttpMethod::Post, url, body)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl BinaryHttpResponse {
    pub fn new(status: u16, headers: BTreeMap<String, String>, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub trait OpenApiTransport: Clone + Send + Sync + 'static {
    fn send_json(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse>>;
}

/// Optional transport capability for bounded binary response bodies.
///
/// This is separate from [`OpenApiTransport`] so existing custom JSON
/// transports do not need to implement binary downloads. Implementations must
/// stop reading and return an error when the response body exceeds
/// `max_response_bytes`.
pub trait OpenApiBinaryTransport: OpenApiTransport {
    fn send_bytes(
        &self,
        request: HttpRequest,
        max_response_bytes: usize,
    ) -> BoxFuture<'static, Result<BinaryHttpResponse>>;
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
            let response = reqwest_request(&client, request)
                .send()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;
            let status = response.status().as_u16();
            if let Some(response) = response_for_error_status(status) {
                return Ok(response);
            }
            let body = response
                .json::<Value>()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;

            Ok(HttpResponse { status, body })
        })
    }
}

#[cfg(feature = "reqwest-transport")]
impl OpenApiBinaryTransport for ReqwestOpenApiTransport {
    fn send_bytes(
        &self,
        request: HttpRequest,
        max_response_bytes: usize,
    ) -> BoxFuture<'static, Result<BinaryHttpResponse>> {
        let client = self.client.clone();

        Box::pin(async move {
            let mut response = reqwest_request(&client, request)
                .send()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_owned(), value.to_owned()))
                })
                .collect();

            if response
                .content_length()
                .is_some_and(|length| length > max_response_bytes as u64)
            {
                return Err(binary_response_too_large(max_response_bytes));
            }

            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?
            {
                let next_length = body
                    .len()
                    .checked_add(chunk.len())
                    .ok_or_else(|| binary_response_too_large(max_response_bytes))?;
                if next_length > max_response_bytes {
                    return Err(binary_response_too_large(max_response_bytes));
                }
                body.extend_from_slice(&chunk);
            }

            Ok(BinaryHttpResponse {
                status,
                headers,
                body,
            })
        })
    }
}

#[cfg(feature = "reqwest-transport")]
fn reqwest_request(client: &reqwest::Client, request: HttpRequest) -> reqwest::RequestBuilder {
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
    builder
}

#[cfg(feature = "reqwest-transport")]
fn binary_response_too_large(max_response_bytes: usize) -> Error {
    Error::Transport(format!(
        "binary response exceeds the {max_response_bytes}-byte limit"
    ))
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

#[cfg(feature = "reqwest-transport")]
fn response_for_error_status(status: u16) -> Option<HttpResponse> {
    (!(200..300).contains(&status)).then_some(HttpResponse {
        status,
        body: Value::Null,
    })
}

#[cfg(all(test, feature = "reqwest-transport"))]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use serde_json::json;

    use super::*;
    use crate::Error;
    use crate::lark_openapi::response::parse_openapi_response;

    #[test]
    fn preserves_error_status_without_requiring_a_json_body() {
        let response = response_for_error_status(503).expect("error response");
        assert_eq!(response.status, 503);
        assert_eq!(response.body, Value::Null);
        assert_eq!(response_for_error_status(200), None);
    }

    #[tokio::test]
    async fn reqwest_classifies_non_json_http_errors_before_body_decoding() {
        for body in ["", "<html>service unavailable</html>"] {
            let response = send_test_response("503 Service Unavailable", "text/html", body).await;
            assert_eq!(response.status, 503);
            assert_eq!(response.body, Value::Null);

            let error = parse_openapi_response::<Value>(response)
                .expect_err("non-success response must retain its HTTP status");
            assert!(matches!(error, Error::HttpStatus { status: 503 }));
        }

        let response = send_test_response(
            "200 OK",
            "application/json",
            r#"{"code":0,"data":{"value":"ok"}}"#,
        )
        .await;
        assert_eq!(
            response.body,
            json!({ "code": 0, "data": { "value": "ok" } })
        );
    }

    #[tokio::test]
    async fn reqwest_preserves_binary_bodies_and_response_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).expect("read request");
            let body = [0_u8, 159, 146, 150];
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Disposition: attachment; filename=\"image.png\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).expect("write headers");
            socket.write_all(&body).expect("write body");
        });

        let response = ReqwestOpenApiTransport::new()
            .send_bytes(
                HttpRequest::empty(
                    HttpMethod::Get,
                    Url::parse(&format!("http://{address}/binary")).expect("test URL"),
                ),
                16,
            )
            .await
            .expect("binary response");
        server.join().expect("test server joined");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, vec![0, 159, 146, 150]);
        assert_eq!(response.header("content-type"), Some("image/png"));
        assert_eq!(
            response.header("content-disposition"),
            Some("attachment; filename=\"image.png\"")
        );
    }

    #[tokio::test]
    async fn reqwest_rejects_binary_content_length_over_the_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).expect("read request");
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody",
                )
                .expect("write response");
        });

        let error = ReqwestOpenApiTransport::new()
            .send_bytes(
                HttpRequest::empty(
                    HttpMethod::Get,
                    Url::parse(&format!("http://{address}/binary")).expect("test URL"),
                ),
                3,
            )
            .await
            .expect_err("oversized response");
        server.join().expect("test server joined");

        assert!(matches!(error, Error::Transport(message) if message.contains("3-byte limit")));
    }

    async fn send_test_response(status: &str, content_type: &str, body: &str) -> HttpResponse {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let status = status.to_owned();
        let content_type = content_type.to_owned();
        let body = body.to_owned();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).expect("read request");
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let response = ReqwestOpenApiTransport::new()
            .send_json(HttpRequest::post_json(
                Url::parse(&format!("http://{address}/openapi")).expect("test URL"),
                json!({ "request": true }),
            ))
            .await
            .expect("transport response");
        server.join().expect("test server joined");
        response
    }
}
