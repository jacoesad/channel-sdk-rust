use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use serde_json::{Value, json};
use url::Url;

use crate::Result;
use crate::lark_openapi::{BoxFuture, HttpMethod, HttpRequest, HttpResponse, OpenApiTransport};

pub(super) fn assert_card_content_update(call: &FakeCall, sequence: u32, content: &str) {
    assert_eq!(call.method, HttpMethod::Put);
    assert!(call.url.as_str().contains("/elements/stream_md/content"));
    assert_eq!(call.body["sequence"], sequence);
    assert_eq!(call.body["content"], content);
    assert!(
        call.body["uuid"]
            .as_str()
            .is_some_and(|uuid| { uuid.starts_with("lc-") && uuid.chars().count() <= 64 })
    );
}

pub(super) fn token_response() -> HttpResponse {
    HttpResponse::json(
        200,
        json!({
            "code": 0,
            "msg": "ok",
            "tenant_access_token": "tenant-token-1",
            "expire": 7200
        }),
    )
}

pub(super) fn card_response(card_id: &str) -> HttpResponse {
    HttpResponse::json(
        200,
        json!({ "code": 0, "msg": "ok", "data": { "card_id": card_id } }),
    )
}

pub(super) fn message_response(message_id: &str) -> HttpResponse {
    HttpResponse::json(
        200,
        json!({ "code": 0, "msg": "ok", "data": { "message_id": message_id } }),
    )
}

pub(super) fn ok_response() -> HttpResponse {
    HttpResponse::json(200, json!({ "code": 0, "msg": "ok", "data": {} }))
}

#[derive(Debug, Clone)]
pub(super) struct FakeTransport {
    state: Arc<Mutex<FakeState>>,
}

impl FakeTransport {
    pub(super) fn new(responses: Vec<Result<HttpResponse>>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                responses: responses.into_iter().map(FakeResponse::from).collect(),
                calls: Vec::new(),
            })),
        }
    }

    pub(super) fn http(responses: Vec<HttpResponse>) -> Self {
        Self::new(responses.into_iter().map(Ok).collect())
    }

    pub(super) fn scripted(responses: Vec<FakeResponse>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                responses: responses.into(),
                calls: Vec::new(),
            })),
        }
    }

    pub(super) fn calls(&self) -> Vec<FakeCall> {
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
                method: request.method,
                url: request.url,
                headers: request.headers,
                body: request.body,
            });
            state.responses.pop_front().expect("fake response")
        };

        match response {
            FakeResponse::Ready(response) => Box::pin(async move { response }),
            FakeResponse::PendingOnce(response) => Box::pin(PendingOnceResponse {
                response: Some(response),
                polled: false,
            }),
        }
    }
}

#[derive(Debug)]
struct FakeState {
    responses: VecDeque<FakeResponse>,
    calls: Vec<FakeCall>,
}

#[derive(Debug)]
pub(super) enum FakeResponse {
    Ready(Result<HttpResponse>),
    PendingOnce(Result<HttpResponse>),
}

impl FakeResponse {
    pub(super) fn ready(response: HttpResponse) -> Self {
        Self::Ready(Ok(response))
    }

    pub(super) fn pending_once(response: HttpResponse) -> Self {
        Self::PendingOnce(Ok(response))
    }
}

impl From<Result<HttpResponse>> for FakeResponse {
    fn from(response: Result<HttpResponse>) -> Self {
        Self::Ready(response)
    }
}

struct PendingOnceResponse {
    response: Option<Result<HttpResponse>>,
    polled: bool,
}

impl Future for PendingOnceResponse {
    type Output = Result<HttpResponse>;

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.polled {
            self.polled = true;
            return Poll::Pending;
        }
        Poll::Ready(
            self.response
                .take()
                .expect("pending response consumed once"),
        )
    }
}

#[derive(Clone, Debug)]
pub(super) struct FakeCall {
    pub(super) method: HttpMethod,
    pub(super) url: Url,
    #[allow(dead_code)]
    headers: BTreeMap<String, String>,
    pub(super) body: Value,
}

pub(super) fn block_on<F>(future: F) -> F::Output
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

pub(super) fn assert_pending_once<F>(future: F)
where
    F: Future,
{
    let waker = noop_waker();
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);

    assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
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
