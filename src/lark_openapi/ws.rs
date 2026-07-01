use serde::{Deserialize, Serialize};
use url::Url;

use crate::{ChannelConfig, Error, Result};

use super::{OpenApiClient, OpenApiTransport};

const WS_ENDPOINT_PATH: &str = "/callback/ws/endpoint";
const DEVICE_ID_QUERY: &str = "device_id";
const SERVICE_ID_QUERY: &str = "service_id";

impl<T> OpenApiClient<T>
where
    T: OpenApiTransport,
{
    /// Request a Feishu/Lark WebSocket endpoint for the current app.
    ///
    /// This mirrors the official SDK long-connection setup endpoint and does
    /// not establish the socket by itself. Use a WebSocket transport to connect
    /// to the returned endpoint.
    pub async fn websocket_endpoint(&self) -> Result<WebSocketEndpoint> {
        let body = WebSocketEndpointRequest::from_config(self.config());
        let response: WebSocketEndpointResponse =
            self.post_openapi_json(WS_ENDPOINT_PATH, &body).await?;
        WebSocketEndpoint::from_payload(response.data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketEndpoint {
    url: Url,
    device_id: String,
    service_id: i32,
    client_config: Option<WebSocketClientConfig>,
}

impl WebSocketEndpoint {
    pub fn new(url: Url, client_config: Option<WebSocketClientConfig>) -> Result<Self> {
        let (device_id, service_id) = parse_websocket_url(&url)?;
        Ok(Self {
            url,
            device_id,
            service_id,
            client_config,
        })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn client_config(&self) -> Option<&WebSocketClientConfig> {
        self.client_config.as_ref()
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn service_id(&self) -> i32 {
        self.service_id
    }

    fn from_payload(payload: WebSocketEndpointPayload) -> Result<Self> {
        let url = Url::parse(&payload.url)?;
        Self::new(url, payload.client_config)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WebSocketClientConfig {
    pub reconnect_count: Option<i32>,
    pub reconnect_interval: Option<u64>,
    pub reconnect_nonce: Option<u64>,
    pub ping_interval: Option<u64>,
}

#[derive(Debug, Serialize)]
struct WebSocketEndpointRequest<'a> {
    #[serde(rename = "AppID")]
    app_id: &'a str,
    #[serde(rename = "AppSecret")]
    app_secret: &'a str,
}

impl<'a> WebSocketEndpointRequest<'a> {
    fn from_config(config: &'a ChannelConfig) -> Self {
        Self {
            app_id: &config.app_id,
            app_secret: &config.app_secret,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WebSocketEndpointResponse {
    data: WebSocketEndpointPayload,
}

#[derive(Debug, Deserialize)]
struct WebSocketEndpointPayload {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "ClientConfig")]
    client_config: Option<WebSocketClientConfig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketFrameMethod {
    Control = 0,
    Data = 1,
}

impl WebSocketFrameMethod {
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Control),
            1 => Some(Self::Data),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebSocketMessageType {
    Event,
    Card,
    Ping,
    Pong,
    Other,
}

impl WebSocketMessageType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Card => "card",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::Other => "other",
        }
    }

    fn from_header(value: Option<&str>) -> Self {
        match value {
            Some("event") => Self::Event,
            Some("card") => Self::Card,
            Some("ping") => Self::Ping,
            Some("pong") => Self::Pong,
            _ => Self::Other,
        }
    }
}

#[cfg_attr(feature = "websocket", derive(::prost::Message))]
#[derive(Clone, PartialEq)]
pub struct WebSocketHeader {
    #[cfg_attr(feature = "websocket", prost(string, tag = "1"))]
    pub key: String,
    #[cfg_attr(feature = "websocket", prost(string, tag = "2"))]
    pub value: String,
}

impl WebSocketHeader {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[cfg_attr(feature = "websocket", derive(::prost::Message))]
#[derive(Clone, PartialEq)]
pub struct WebSocketFrame {
    #[cfg_attr(feature = "websocket", prost(uint64, tag = "1"))]
    pub seq_id: u64,
    #[cfg_attr(feature = "websocket", prost(uint64, tag = "2"))]
    pub log_id: u64,
    #[cfg_attr(feature = "websocket", prost(int32, tag = "3"))]
    pub service: i32,
    #[cfg_attr(feature = "websocket", prost(int32, tag = "4"))]
    pub method: i32,
    #[cfg_attr(feature = "websocket", prost(message, repeated, tag = "5"))]
    pub headers: Vec<WebSocketHeader>,
    #[cfg_attr(feature = "websocket", prost(string, optional, tag = "6"))]
    pub payload_encoding: Option<String>,
    #[cfg_attr(feature = "websocket", prost(string, optional, tag = "7"))]
    pub payload_type: Option<String>,
    #[cfg_attr(feature = "websocket", prost(bytes = "vec", optional, tag = "8"))]
    pub payload: Option<Vec<u8>>,
    #[cfg_attr(feature = "websocket", prost(string, optional, tag = "9"))]
    pub log_id_new: Option<String>,
}

impl WebSocketFrame {
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.key == key)
            .map(|header| header.value.as_str())
    }

    pub fn method(&self) -> Option<WebSocketFrameMethod> {
        WebSocketFrameMethod::from_i32(self.method)
    }

    pub fn message_type(&self) -> WebSocketMessageType {
        WebSocketMessageType::from_header(self.header("type"))
    }

    pub fn event_payload(&self) -> Option<&[u8]> {
        (self.method() == Some(WebSocketFrameMethod::Data)
            && self.message_type() == WebSocketMessageType::Event)
            .then_some(self.payload.as_deref())
            .flatten()
    }
}

#[cfg(feature = "websocket")]
impl WebSocketFrame {
    pub fn encode_to_vec(&self) -> Vec<u8> {
        ::prost::Message::encode_to_vec(self)
    }

    pub fn decode(bytes: impl AsRef<[u8]>) -> Result<Self> {
        <Self as ::prost::Message>::decode(bytes.as_ref())
            .map_err(|error| Error::Transport(format!("invalid websocket frame: {error}")))
    }
}

#[cfg(feature = "websocket")]
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioTungsteniteWebSocketTransport;

#[cfg(feature = "websocket")]
impl TokioTungsteniteWebSocketTransport {
    pub fn new() -> Self {
        Self
    }

    pub async fn connect(&self, endpoint: &WebSocketEndpoint) -> Result<WebSocketConnection> {
        WebSocketConnection::connect(endpoint).await
    }
}

#[cfg(feature = "websocket")]
pub struct WebSocketConnection {
    url: Url,
    device_id: String,
    service_id: i32,
    stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

#[cfg(feature = "websocket")]
impl WebSocketConnection {
    pub async fn connect(endpoint: &WebSocketEndpoint) -> Result<Self> {
        use tokio_tungstenite::connect_async;

        let (stream, _) = connect_async(endpoint.url().as_str())
            .await
            .map_err(|error| Error::Transport(format!("websocket connect failed: {error}")))?;
        Ok(Self {
            url: endpoint.url().clone(),
            device_id: endpoint.device_id().to_owned(),
            service_id: endpoint.service_id(),
            stream,
        })
    }

    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn service_id(&self) -> i32 {
        self.service_id
    }

    pub async fn next_frame(&mut self) -> Result<Option<WebSocketFrame>> {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        while let Some(message) = self.stream.next().await {
            let message = message
                .map_err(|error| Error::Transport(format!("websocket receive failed: {error}")))?;
            match message {
                Message::Binary(bytes) => return WebSocketFrame::decode(bytes).map(Some),
                Message::Close(_) => return Ok(None),
                Message::Ping(bytes) => {
                    self.stream
                        .send(Message::Pong(bytes))
                        .await
                        .map_err(|error| {
                            Error::Transport(format!("websocket pong failed: {error}"))
                        })?;
                }
                Message::Text(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
        Ok(None)
    }

    pub async fn send_frame(&mut self, frame: &WebSocketFrame) -> Result<()> {
        use futures_util::SinkExt;
        use tokio_tungstenite::tungstenite::Message;

        self.stream
            .send(Message::Binary(frame.encode_to_vec().into()))
            .await
            .map_err(|error| Error::Transport(format!("websocket send failed: {error}")))
    }

    pub async fn close(mut self) -> Result<()> {
        self.stream
            .close(None)
            .await
            .map_err(|error| Error::Transport(format!("websocket close failed: {error}")))
    }
}

fn query_value(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, value)| value.into_owned())
}

fn parse_websocket_url(url: &Url) -> Result<(String, i32)> {
    match url.scheme() {
        "ws" | "wss" => {}
        scheme => {
            return Err(Error::Validation(format!(
                "websocket endpoint must use ws or wss, got {scheme}"
            )));
        }
    }
    let device_id = query_value(url, DEVICE_ID_QUERY)
        .ok_or_else(|| Error::Validation("websocket endpoint is missing device_id".to_owned()))?;
    let service_id_value = query_value(url, SERVICE_ID_QUERY)
        .ok_or_else(|| Error::Validation("websocket endpoint is missing service_id".to_owned()))?;
    let service_id = service_id_value.parse::<i32>().map_err(|_| {
        Error::Validation(format!(
            "websocket endpoint service_id must be an integer, got {service_id_value}"
        ))
    })?;
    Ok((device_id, service_id))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::future::Future;
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use serde_json::{Value, json};

    use super::*;
    use crate::lark_openapi::{BoxFuture, HttpResponse};
    use crate::{ChannelConfig, Result};

    #[test]
    fn websocket_endpoint_requests_sdk_endpoint() {
        let transport = FakeTransport::new(vec![HttpResponse::json(
            200,
            json!({
                "code": 0,
                "data": {
                    "URL": "wss://example.test/callback?device_id=device&service_id=42",
                    "ClientConfig": {
                        "ReconnectCount": 3,
                        "ReconnectInterval": 10,
                        "ReconnectNonce": 2,
                        "PingInterval": 30
                    }
                }
            }),
        )]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let endpoint = block_on(client.websocket_endpoint()).expect("endpoint");

        assert_eq!(
            endpoint.url().as_str(),
            "wss://example.test/callback?device_id=device&service_id=42"
        );
        assert_eq!(endpoint.device_id(), "device");
        assert_eq!(endpoint.service_id(), 42);
        assert_eq!(
            endpoint.client_config(),
            Some(&WebSocketClientConfig {
                reconnect_count: Some(3),
                reconnect_interval: Some(10),
                reconnect_nonce: Some(2),
                ping_interval: Some(30),
            })
        );

        let calls = transport.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].url.as_str(),
            "https://open.feishu.cn/callback/ws/endpoint"
        );
        assert_eq!(
            calls[0].body,
            json!({"AppID": "cli_a", "AppSecret": "secret"})
        );
        assert_eq!(
            calls[0].headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn websocket_endpoint_validates_required_query_values() {
        let url = Url::parse("wss://example.test/callback?device_id=device").expect("url");

        let error = WebSocketEndpoint::new(url, None).expect_err("missing service id");

        assert!(matches!(error, Error::Validation(message) if message.contains("service_id")));
    }

    #[test]
    fn websocket_endpoint_rejects_invalid_service_id() {
        let url =
            Url::parse("wss://example.test/callback?device_id=device&service_id=abc").expect("url");

        let error = WebSocketEndpoint::new(url, None).expect_err("invalid service id");

        assert!(
            matches!(error, Error::Validation(message) if message.contains("must be an integer"))
        );
    }

    #[test]
    fn websocket_frame_exposes_headers_and_event_payload() {
        let frame = WebSocketFrame {
            seq_id: 1,
            log_id: 2,
            service: 42,
            method: WebSocketFrameMethod::Data as i32,
            headers: vec![
                WebSocketHeader::new("type", "event"),
                WebSocketHeader::new("message_id", "om_1"),
            ],
            payload_encoding: None,
            payload_type: Some("application/json".to_owned()),
            payload: Some(br#"{"schema":"2.0"}"#.to_vec()),
            log_id_new: Some("log-new".to_owned()),
        };

        assert_eq!(frame.header("message_id"), Some("om_1"));
        assert_eq!(frame.method(), Some(WebSocketFrameMethod::Data));
        assert_eq!(frame.message_type(), WebSocketMessageType::Event);
        assert_eq!(
            frame.event_payload(),
            Some(br#"{"schema":"2.0"}"#.as_slice())
        );
    }

    #[cfg(feature = "websocket")]
    #[test]
    fn websocket_frame_round_trips_protobuf_binary() {
        let frame = WebSocketFrame {
            seq_id: 1,
            log_id: 2,
            service: 42,
            method: WebSocketFrameMethod::Data as i32,
            headers: vec![WebSocketHeader::new("type", "event")],
            payload_encoding: None,
            payload_type: None,
            payload: Some(b"payload".to_vec()),
            log_id_new: None,
        };

        let decoded = WebSocketFrame::decode(frame.encode_to_vec()).expect("decoded");

        assert_eq!(decoded.seq_id, 1);
        assert_eq!(decoded.log_id, 2);
        assert_eq!(decoded.service, 42);
        assert_eq!(decoded.method(), Some(WebSocketFrameMethod::Data));
        assert_eq!(decoded.header("type"), Some("event"));
        assert_eq!(decoded.payload.as_deref(), Some(b"payload".as_slice()));
    }

    #[derive(Clone)]
    struct FakeTransport {
        state: Arc<Mutex<FakeState>>,
    }

    impl FakeTransport {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                state: Arc::new(Mutex::new(FakeState {
                    responses: responses.into(),
                    calls: Vec::new(),
                })),
            }
        }

        fn calls(&self) -> Vec<FakeCall> {
            self.state().calls.clone()
        }

        fn state(&self) -> MutexGuard<'_, FakeState> {
            self.state.lock().expect("fake transport state poisoned")
        }
    }

    impl OpenApiTransport for FakeTransport {
        fn send_json(
            &self,
            request: super::super::HttpRequest,
        ) -> BoxFuture<'static, Result<HttpResponse>> {
            let response = {
                let mut state = self.state();
                state.calls.push(FakeCall {
                    url: request.url,
                    headers: request.headers,
                    body: request.body,
                });
                state.responses.pop_front().expect("fake response")
            };

            Box::pin(async move { Ok(response) })
        }
    }

    #[derive(Debug)]
    struct FakeState {
        responses: VecDeque<HttpResponse>,
        calls: Vec<FakeCall>,
    }

    #[derive(Clone, Debug)]
    struct FakeCall {
        url: Url,
        headers: BTreeMap<String, String>,
        body: Value,
    }

    fn block_on<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        unsafe { Waker::from_raw(raw) }
    }
}
