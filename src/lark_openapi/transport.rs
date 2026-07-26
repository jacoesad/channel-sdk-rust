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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartRequest {
    pub method: HttpMethod,
    pub url: Url,
    pub headers: BTreeMap<String, String>,
    pub parts: Vec<MultipartPart>,
}

impl MultipartRequest {
    pub fn new(method: HttpMethod, url: Url) -> Self {
        Self {
            method,
            url,
            headers: BTreeMap::new(),
            parts: Vec::new(),
        }
    }

    pub fn with_bearer_auth(mut self, token: impl Into<String>) -> Self {
        self.headers.insert(
            "authorization".to_owned(),
            format!("Bearer {}", token.into()),
        );
        self
    }

    pub fn text(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.parts.push(MultipartPart::Text {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    pub fn file(
        mut self,
        name: impl Into<String>,
        file_name: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        self.parts.push(MultipartPart::File {
            name: name.into(),
            file_name: file_name.into(),
            bytes,
        });
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultipartPart {
    Text {
        name: String,
        value: String,
    },
    File {
        name: String,
        file_name: String,
        bytes: Vec<u8>,
    },
}

pub trait OpenApiTransport: Clone + Send + Sync + 'static {
    fn send_json(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse>>;
}

/// Optional transport capability for bounded binary response bodies.
///
/// This is separate from [`OpenApiTransport`] so existing custom JSON
/// transports do not need to implement binary downloads. Implementations must
/// preserve a non-success HTTP status without buffering its response body or
/// applying `max_response_bytes`. For successful responses, they must stop
/// reading and return an error when the response body exceeds
/// `max_response_bytes`.
pub trait OpenApiBinaryTransport: OpenApiTransport {
    fn send_bytes(
        &self,
        request: HttpRequest,
        max_response_bytes: usize,
    ) -> BoxFuture<'static, Result<BinaryHttpResponse>>;
}

/// Optional transport capability for multipart OpenAPI requests.
///
/// This remains separate from [`OpenApiTransport`] so existing custom JSON
/// transports do not need to implement media uploads.
pub trait OpenApiMultipartTransport: OpenApiTransport {
    fn send_multipart(&self, request: MultipartRequest)
    -> BoxFuture<'static, Result<HttpResponse>>;
}

#[cfg(feature = "reqwest-transport")]
#[derive(Debug, Clone)]
pub struct ReqwestOpenApiTransport {
    client: reqwest::Client,
    binary_client: std::result::Result<reqwest::Client, String>,
}

#[cfg(feature = "reqwest-transport")]
impl ReqwestOpenApiTransport {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            binary_client: binary_client(),
        }
    }

    /// Uses `client` for JSON and multipart requests.
    ///
    /// Binary resource downloads use a dedicated client with redirects
    /// disabled so the original OpenAPI status cannot be replaced by a
    /// redirected response.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            client,
            binary_client: binary_client(),
        }
    }

    /// Builds independently configured JSON/multipart and binary clients.
    ///
    /// Redirects are always disabled for the binary client so message-resource
    /// downloads preserve the original OpenAPI response status. Configure both
    /// builders when network policy such as custom roots, proxies, or timeouts
    /// must apply to every transport capability.
    pub fn with_client_builders(
        client_builder: reqwest::ClientBuilder,
        binary_client_builder: reqwest::ClientBuilder,
    ) -> Result<Self> {
        let client = client_builder.build().map_err(|error| {
            Error::Transport(format!("failed to build Reqwest client: {error}"))
        })?;
        let binary_client = build_binary_client(binary_client_builder).map_err(Error::Transport)?;

        Ok(Self {
            client,
            binary_client: Ok(binary_client),
        })
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
        let client = self.binary_client.clone();

        Box::pin(async move {
            let client = client.map_err(Error::Transport)?;
            let mut response = reqwest_request(&client, request)
                .send()
                .await
                .map_err(|error| Error::Transport(error.to_string()))?;
            let status = response.status().as_u16();
            if let Some(response) = binary_response_for_error_status(status) {
                return Ok(response);
            }
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
impl OpenApiMultipartTransport for ReqwestOpenApiTransport {
    fn send_multipart(
        &self,
        request: MultipartRequest,
    ) -> BoxFuture<'static, Result<HttpResponse>> {
        let client = self.client.clone();

        Box::pin(async move {
            let MultipartRequest {
                method,
                url,
                headers,
                parts,
            } = request;

            let mut form = reqwest::multipart::Form::new();
            for part in parts {
                form = match part {
                    MultipartPart::Text { name, value } => form.text(name, value),
                    MultipartPart::File {
                        name,
                        file_name,
                        bytes,
                    } => form.part(
                        name,
                        reqwest::multipart::Part::bytes(bytes).file_name(file_name),
                    ),
                };
            }

            let mut builder = client.request(method.into(), url);
            for (name, value) in headers {
                builder = builder.header(name, value);
            }
            let response = builder
                .multipart(form)
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
fn binary_client() -> std::result::Result<reqwest::Client, String> {
    build_binary_client(reqwest::Client::builder())
}

#[cfg(feature = "reqwest-transport")]
fn build_binary_client(
    builder: reqwest::ClientBuilder,
) -> std::result::Result<reqwest::Client, String> {
    builder
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("failed to build binary Reqwest client: {error}"))
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

#[cfg(feature = "reqwest-transport")]
fn binary_response_for_error_status(status: u16) -> Option<BinaryHttpResponse> {
    (!(200..300).contains(&status)).then_some(BinaryHttpResponse::new(
        status,
        BTreeMap::new(),
        Vec::new(),
    ))
}

#[cfg(all(test, feature = "reqwest-transport"))]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
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

        let response = ReqwestOpenApiTransport::with_client(reqwest::Client::new())
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
    async fn reqwest_client_builders_apply_binary_client_settings() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let captured = Arc::new(Mutex::new(Vec::new()));
        let server_capture = captured.clone();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let request = read_http_request(&mut socket);
            *server_capture.lock().expect("capture state") = request;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("write response");
        });

        let mut default_headers = reqwest::header::HeaderMap::new();
        default_headers.insert(
            "x-binary-client",
            reqwest::header::HeaderValue::from_static("configured"),
        );
        let transport = ReqwestOpenApiTransport::with_client_builders(
            reqwest::Client::builder(),
            reqwest::Client::builder().default_headers(default_headers),
        )
        .expect("configured transport");

        let response = transport
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

        assert_eq!(response.body, b"ok");
        let request = captured.lock().expect("capture state");
        let request = String::from_utf8_lossy(&request);
        assert!(request.contains("x-binary-client: configured\r\n"));
    }

    #[tokio::test]
    async fn reqwest_client_builders_apply_regular_client_settings() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let captured = Arc::new(Mutex::new(Vec::new()));
        let server_capture = captured.clone();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let request = read_http_request(&mut socket);
            *server_capture.lock().expect("capture state") = request;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                )
                .expect("write response");
        });

        let mut default_headers = reqwest::header::HeaderMap::new();
        default_headers.insert(
            "x-regular-client",
            reqwest::header::HeaderValue::from_static("configured"),
        );
        let transport = ReqwestOpenApiTransport::with_client_builders(
            reqwest::Client::builder().default_headers(default_headers),
            reqwest::Client::builder(),
        )
        .expect("configured transport");

        let response = transport
            .send_json(HttpRequest::empty(
                HttpMethod::Get,
                Url::parse(&format!("http://{address}/json")).expect("test URL"),
            ))
            .await
            .expect("JSON response");
        server.join().expect("test server joined");

        assert_eq!(response.body, json!({}));
        let request = captured.lock().expect("capture state");
        let request = String::from_utf8_lossy(&request);
        assert!(request.contains("x-regular-client: configured\r\n"));
    }

    #[tokio::test]
    async fn reqwest_serializes_multipart_text_and_file_parts() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let captured = Arc::new(Mutex::new(Vec::new()));
        let server_capture = captured.clone();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let request = read_http_request(&mut socket);
            *server_capture.lock().expect("capture state") = request;
            let body = r#"{"code":0,"data":{"image_key":"img_123"}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let response = ReqwestOpenApiTransport::new()
            .send_multipart(
                MultipartRequest::new(
                    HttpMethod::Post,
                    Url::parse(&format!("http://{address}/upload")).expect("test URL"),
                )
                .with_bearer_auth("tenant-token")
                .text("image_type", "message")
                .file("image", "image.png", b"png-data".to_vec()),
            )
            .await
            .expect("multipart response");
        server.join().expect("test server joined");

        assert_eq!(response.status, 200);
        assert_eq!(
            response.body,
            json!({"code": 0, "data": {"image_key": "img_123"}})
        );

        let request = captured.lock().expect("capture state");
        let request = String::from_utf8_lossy(&request);
        assert!(request.starts_with("POST /upload HTTP/1.1\r\n"));
        assert!(request.contains("authorization: Bearer tenant-token\r\n"));
        assert!(request.contains("content-type: multipart/form-data; boundary="));
        assert!(request.contains("name=\"image_type\""));
        assert!(request.contains("\r\n\r\nmessage\r\n"));
        assert!(request.contains("name=\"image\"; filename=\"image.png\""));
        assert!(request.contains("\r\n\r\npng-data\r\n"));
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

    #[tokio::test]
    async fn reqwest_rejects_chunked_binary_body_over_the_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).expect("read request");
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nab\r\n2\r\ncd\r\n0\r\n\r\n",
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

    #[tokio::test]
    async fn reqwest_preserves_binary_http_errors_before_body_limits() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bound test server");
        let address = listener.local_addr().expect("test server address");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).expect("read request");
            socket
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/html\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n",
                )
                .expect("write response headers");
        });

        let response = ReqwestOpenApiTransport::new()
            .send_bytes(
                HttpRequest::empty(
                    HttpMethod::Get,
                    Url::parse(&format!("http://{address}/binary")).expect("test URL"),
                ),
                3,
            )
            .await
            .expect("HTTP status response");
        server.join().expect("test server joined");

        assert_eq!(response.status, 503);
        assert!(response.body.is_empty());
    }

    #[tokio::test]
    async fn reqwest_binary_clients_do_not_follow_redirects() {
        assert_binary_redirect_is_not_followed(ReqwestOpenApiTransport::new()).await;
        assert_binary_redirect_is_not_followed(ReqwestOpenApiTransport::with_client(
            reqwest::Client::new(),
        ))
        .await;
        assert_binary_redirect_is_not_followed(
            ReqwestOpenApiTransport::with_client_builders(
                reqwest::Client::builder(),
                reqwest::Client::builder(),
            )
            .expect("configured transport"),
        )
        .await;
    }

    async fn assert_binary_redirect_is_not_followed(transport: ReqwestOpenApiTransport) {
        let target_listener = TcpListener::bind("127.0.0.1:0").expect("bound redirect target");
        target_listener
            .set_nonblocking(true)
            .expect("nonblocking redirect target");
        let target_address = target_listener
            .local_addr()
            .expect("redirect target address");
        let redirected = Arc::new(AtomicBool::new(false));
        let target_done = Arc::new(AtomicBool::new(false));
        let target_redirected = redirected.clone();
        let target_stop = target_done.clone();
        let target = thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while !target_stop.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
                match target_listener.accept() {
                    Ok((mut socket, _)) => {
                        target_redirected.store(true, Ordering::Release);
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nredirected",
                            )
                            .expect("write redirect target response");
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::yield_now();
                    }
                    Err(error) => panic!("accept redirect target: {error}"),
                }
            }
        });

        let listener = TcpListener::bind("127.0.0.1:0").expect("bound redirect server");
        let address = listener.local_addr().expect("redirect server address");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accepted request");
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).expect("read request");
            let response = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{target_address}/redirected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            socket
                .write_all(response.as_bytes())
                .expect("write redirect response");
        });

        let response = transport
            .send_bytes(
                HttpRequest::empty(
                    HttpMethod::Get,
                    Url::parse(&format!("http://{address}/binary")).expect("test URL"),
                ),
                16,
            )
            .await
            .expect("redirect status response");
        server.join().expect("redirect server joined");
        target_done.store(true, Ordering::Release);
        target.join().expect("redirect target joined");

        assert_eq!(response.status, 302);
        assert!(response.body.is_empty());
        assert!(!redirected.load(Ordering::Acquire));
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

    fn read_http_request(socket: &mut std::net::TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut expected_length = None;

        loop {
            let read = socket.read(&mut buffer).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);

            if expected_length.is_none() {
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let body_length = headers
                        .lines()
                        .find_map(|line| {
                            line.split_once(':').and_then(|(name, value)| {
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().expect("content length"))
                            })
                        })
                        .unwrap_or(0);
                    expected_length = Some(header_end + 4 + body_length);
                }
            }

            if expected_length.is_some_and(|length| request.len() >= length) {
                break;
            }
        }

        request
    }
}
