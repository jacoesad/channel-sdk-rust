use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use crate::message::{MessageContent, MessageId, Recipient};
use crate::{ChannelConfig, Error, Result};

const APP_ACCESS_TOKEN_PATH: &str = "/open-apis/auth/v3/app_access_token/internal";
const TENANT_ACCESS_TOKEN_PATH: &str = "/open-apis/auth/v3/tenant_access_token/internal";
const SEND_MESSAGE_PATH: &str = "/open-apis/im/v1/messages";
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(600);

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

    pub async fn app_access_token(&self) -> Result<String> {
        let now = Instant::now();
        if let Some(token) = self.app_access_token_cache.get(now) {
            return Ok(token);
        }

        let token = self.request_app_access_token().await?;
        let access_token = token.app_access_token.clone();
        self.app_access_token_cache
            .store(access_token.clone(), token.expire, Instant::now());
        Ok(access_token)
    }

    pub async fn tenant_access_token(&self) -> Result<String> {
        let now = Instant::now();
        if let Some(token) = self.tenant_access_token_cache.get(now) {
            return Ok(token);
        }

        let token = self.request_tenant_access_token().await?;
        let access_token = token.tenant_access_token.clone();
        self.tenant_access_token_cache
            .store(access_token.clone(), token.expire, Instant::now());
        Ok(access_token)
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

    pub async fn send_message(
        &self,
        recipient: Recipient,
        content: MessageContent,
    ) -> Result<MessageId> {
        let recipient = SendMessageRecipient::try_from(recipient)?;
        let content = SendMessageContent::try_from(content)?;
        let path = format!(
            "{}?receive_id_type={}",
            SEND_MESSAGE_PATH, recipient.receive_id_type
        );
        let request = SendMessageRequest {
            receive_id: recipient.receive_id,
            msg_type: content.msg_type,
            content: serde_json::to_string(&content.content)?,
        };
        let response: SendMessageResponse = self.post_tenant_json(&path, &request).await?;

        Ok(MessageId(response.data.message_id))
    }

    pub async fn send_text_message(
        &self,
        recipient: Recipient,
        text: impl Into<String>,
    ) -> Result<MessageId> {
        self.send_message(recipient, MessageContent::Text { text: text.into() })
            .await
    }

    async fn request_app_access_token(&self) -> Result<AppAccessTokenResponse> {
        self.post_openapi_json(
            APP_ACCESS_TOKEN_PATH,
            &SelfBuiltTokenRequest {
                app_id: &self.config.app_id,
                app_secret: &self.config.app_secret,
            },
        )
        .await
    }

    async fn request_tenant_access_token(&self) -> Result<TenantAccessTokenResponse> {
        self.post_openapi_json(
            TENANT_ACCESS_TOKEN_PATH,
            &SelfBuiltTokenRequest {
                app_id: &self.config.app_id,
                app_secret: &self.config.app_secret,
            },
        )
        .await
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AppAccessTokenResponse {
    pub app_access_token: String,
    pub expire: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct TenantAccessTokenResponse {
    pub tenant_access_token: String,
    pub expire: u64,
}

#[derive(Debug, Serialize)]
struct SelfBuiltTokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

#[derive(Debug)]
struct SendMessageRecipient {
    receive_id_type: &'static str,
    receive_id: String,
}

impl TryFrom<Recipient> for SendMessageRecipient {
    type Error = Error;

    fn try_from(recipient: Recipient) -> Result<Self> {
        match recipient {
            Recipient::Chat(receive_id) => Ok(Self {
                receive_id_type: "chat_id",
                receive_id,
            }),
            Recipient::User(receive_id) => Ok(Self {
                receive_id_type: "open_id",
                receive_id,
            }),
            Recipient::OpenMessage(_) => Err(Error::Config(
                "open message recipients require the reply message API".to_owned(),
            )),
        }
    }
}

#[derive(Debug)]
struct SendMessageContent {
    msg_type: String,
    content: Value,
}

impl TryFrom<MessageContent> for SendMessageContent {
    type Error = Error;

    fn try_from(content: MessageContent) -> Result<Self> {
        match content {
            MessageContent::Text { text } => Ok(Self {
                msg_type: "text".to_owned(),
                content: serde_json::json!({ "text": text }),
            }),
            MessageContent::Card { card } => Ok(Self {
                msg_type: "interactive".to_owned(),
                content: card,
            }),
            MessageContent::Custom { msg_type, content } => Ok(Self { msg_type, content }),
        }
    }
}

#[derive(Debug, Serialize)]
struct SendMessageRequest {
    receive_id: String,
    msg_type: String,
    content: String,
}

#[derive(Debug, serde::Deserialize)]
struct SendMessageResponse {
    data: SendMessageData,
}

#[derive(Debug, serde::Deserialize)]
struct SendMessageData {
    message_id: String,
}

fn parse_openapi_response<R>(response: HttpResponse) -> Result<R>
where
    R: DeserializeOwned,
{
    if let Some(code) = response.body.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let message = response
                .body
                .get("msg")
                .or_else(|| response.body.get("message"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            return Err(Error::Api { code, message });
        }
    }

    if !(200..300).contains(&response.status) {
        return Err(Error::Transport(format!(
            "http status {} from OpenAPI",
            response.status
        )));
    }

    serde_json::from_value(response.body).map_err(Error::from)
}

#[derive(Debug)]
struct AccessTokenCache {
    token: Mutex<Option<CachedAccessToken>>,
}

impl Default for AccessTokenCache {
    fn default() -> Self {
        Self {
            token: Mutex::new(None),
        }
    }
}

impl AccessTokenCache {
    fn get(&self, now: Instant) -> Option<String> {
        let token = self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let token = token.as_ref()?;

        if token.expires_at > now + TOKEN_REFRESH_SKEW {
            Some(token.value.clone())
        } else {
            None
        }
    }

    fn store(&self, value: String, expire: u64, now: Instant) {
        let mut token = self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *token = Some(CachedAccessToken {
            value,
            expires_at: now + Duration::from_secs(expire),
        });
    }
}

#[derive(Debug)]
struct CachedAccessToken {
    value: String,
    expires_at: Instant,
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::MutexGuard;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use serde_json::json;

    use super::*;

    #[test]
    fn app_access_token_requests_and_caches_token() {
        let transport = FakeTransport::new(vec![HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "app_access_token": "token-1",
                "expire": 7200
            }),
        )]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let first = block_on(client.app_access_token()).expect("first token");
        let second = block_on(client.app_access_token()).expect("cached token");

        assert_eq!(first, "token-1");
        assert_eq!(second, "token-1");

        let calls = transport.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].url.as_str(),
            "https://open.feishu.cn/open-apis/auth/v3/app_access_token/internal"
        );
        assert_eq!(
            calls[0].body,
            json!({
                "app_id": "cli_a",
                "app_secret": "secret"
            })
        );
    }

    #[test]
    fn app_access_token_refreshes_when_token_is_inside_refresh_skew() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "app_access_token": "token-1",
                    "expire": 1
                }),
            ),
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "app_access_token": "token-2",
                    "expire": 7200
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let first = block_on(client.app_access_token()).expect("first token");
        let second = block_on(client.app_access_token()).expect("refreshed token");

        assert_eq!(first, "token-1");
        assert_eq!(second, "token-2");
        assert_eq!(transport.calls().len(), 2);
    }

    #[test]
    fn tenant_access_token_requests_and_caches_token() {
        let transport = FakeTransport::new(vec![HttpResponse::json(
            200,
            json!({
                "code": 0,
                "msg": "ok",
                "tenant_access_token": "tenant-token-1",
                "expire": 7200
            }),
        )]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let first = block_on(client.tenant_access_token()).expect("first tenant token");
        let second = block_on(client.tenant_access_token()).expect("cached tenant token");

        assert_eq!(first, "tenant-token-1");
        assert_eq!(second, "tenant-token-1");

        let calls = transport.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].url.as_str(),
            "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal"
        );
        assert_eq!(
            calls[0].body,
            json!({
                "app_id": "cli_a",
                "app_secret": "secret"
            })
        );
    }

    #[test]
    fn post_tenant_json_adds_bearer_token() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_123"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let response = block_on(client.post_tenant_json::<_, Value>(
            "/open-apis/im/v1/messages",
            &json!({
                "receive_id": "oc_123",
                "msg_type": "text"
            }),
        ))
        .expect("tenant request");

        assert_eq!(
            response,
            json!({
                "code": 0,
                "msg": "ok",
                "data": {
                    "message_id": "om_123"
                }
            })
        );

        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages"
        );
        assert_eq!(
            calls[1].headers.get("authorization").map(String::as_str),
            Some("Bearer tenant-token-1")
        );
        assert_eq!(
            calls[1].headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn send_text_message_posts_tenant_message() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_123"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let message_id = block_on(
            client.send_text_message(Recipient::Chat("oc_123".to_owned()), "hello from rust"),
        )
        .expect("sent message");

        assert_eq!(message_id, MessageId("om_123".to_owned()));

        let calls = transport.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id"
        );
        assert_eq!(
            calls[1].headers.get("authorization").map(String::as_str),
            Some("Bearer tenant-token-1")
        );
        assert_eq!(
            calls[1].body,
            json!({
                "receive_id": "oc_123",
                "msg_type": "text",
                "content": "{\"text\":\"hello from rust\"}"
            })
        );
    }

    #[test]
    fn send_message_maps_user_recipient_to_open_id() {
        let transport = FakeTransport::new(vec![
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "tenant_access_token": "tenant-token-1",
                    "expire": 7200
                }),
            ),
            HttpResponse::json(
                200,
                json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "message_id": "om_123"
                    }
                }),
            ),
        ]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        block_on(client.send_message(
            Recipient::User("ou_123".to_owned()),
            MessageContent::Custom {
                msg_type: "text".to_owned(),
                content: json!({ "text": "hello" }),
            },
        ))
        .expect("sent message");

        let calls = transport.calls();
        assert_eq!(
            calls[1].url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=open_id"
        );
        assert_eq!(
            calls[1].body,
            json!({
                "receive_id": "ou_123",
                "msg_type": "text",
                "content": "{\"text\":\"hello\"}"
            })
        );
    }

    #[test]
    fn send_message_rejects_open_message_recipient() {
        let transport = FakeTransport::new(vec![]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport.clone());

        let error = block_on(
            client.send_text_message(Recipient::OpenMessage("om_123".to_owned()), "reply"),
        )
        .expect_err("unsupported recipient");

        assert!(matches!(
            error,
            Error::Config(message) if message == "open message recipients require the reply message API"
        ));
        assert!(transport.calls().is_empty());
    }

    #[test]
    fn post_openapi_json_returns_typed_api_error() {
        let transport = FakeTransport::new(vec![HttpResponse::json(
            200,
            json!({
                "code": 99991663,
                "msg": "invalid app secret"
            }),
        )]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

        let error = block_on(client.app_access_token()).expect_err("api error");

        assert!(matches!(
            error,
            Error::Api {
                code: 99991663,
                message
            } if message == "invalid app secret"
        ));
    }

    #[test]
    fn post_openapi_json_accepts_message_alias_for_api_error() {
        let transport = FakeTransport::new(vec![HttpResponse::json(
            200,
            json!({
                "code": 99991663,
                "message": "invalid app secret"
            }),
        )]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

        let error = block_on(client.app_access_token()).expect_err("api error");

        assert!(matches!(
            error,
            Error::Api {
                code: 99991663,
                message
            } if message == "invalid app secret"
        ));
    }

    #[test]
    fn post_openapi_json_returns_transport_error_for_non_success_status() {
        let transport = FakeTransport::new(vec![HttpResponse::json(
            500,
            json!({
                "code": 0,
                "msg": "ok"
            }),
        )]);
        let client = OpenApiClient::new(ChannelConfig::new("cli_a", "secret"), transport);

        let error =
            block_on(client.post_openapi_json::<_, Value>("/open-apis/example", &json!({})))
                .expect_err("http status error");

        assert!(matches!(
            error,
            Error::Transport(message) if message == "http status 500 from OpenAPI"
        ));
    }

    #[derive(Clone, Debug)]
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
        fn send_json(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse>> {
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
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    fn noop_waker() -> Waker {
        unsafe { Waker::from_raw(noop_raw_waker()) }
    }

    fn noop_raw_waker() -> RawWaker {
        fn clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }

        fn wake(_: *const ()) {}
        fn wake_by_ref(_: *const ()) {}
        fn drop(_: *const ()) {}

        RawWaker::new(
            std::ptr::null(),
            &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
        )
    }
}
