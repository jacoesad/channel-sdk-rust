use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use serde_json::Value;
use url::Url;

use crate::{Error, Result};

use super::{
    BinaryHttpResponse, BoxFuture, HttpMethod, HttpRequest, HttpResponse, OpenApiBinaryTransport,
    OpenApiTransport,
};

#[derive(Clone, Debug)]
pub(super) struct FakeTransport {
    state: Arc<Mutex<FakeState>>,
}

impl FakeTransport {
    pub(super) fn new(responses: Vec<HttpResponse>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                responses: responses.into(),
                binary_responses: VecDeque::new(),
                calls: Vec::new(),
            })),
        }
    }

    pub(super) fn with_binary_responses(
        responses: Vec<HttpResponse>,
        binary_responses: Vec<BinaryHttpResponse>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                responses: responses.into(),
                binary_responses: binary_responses.into(),
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
                max_response_bytes: None,
            });
            state.responses.pop_front().expect("fake response")
        };

        Box::pin(async move { Ok(response) })
    }
}

impl OpenApiBinaryTransport for FakeTransport {
    fn send_bytes(
        &self,
        request: HttpRequest,
        max_response_bytes: usize,
    ) -> BoxFuture<'static, Result<BinaryHttpResponse>> {
        let response = {
            let mut state = self.state();
            state.calls.push(FakeCall {
                method: request.method,
                url: request.url,
                headers: request.headers,
                body: request.body,
                max_response_bytes: Some(max_response_bytes),
            });
            state
                .binary_responses
                .pop_front()
                .expect("fake binary response")
        };

        Box::pin(async move {
            if response.body.len() > max_response_bytes {
                return Err(Error::Transport(format!(
                    "binary response exceeds the {max_response_bytes}-byte limit"
                )));
            }
            Ok(response)
        })
    }
}

#[derive(Debug)]
struct FakeState {
    responses: VecDeque<HttpResponse>,
    binary_responses: VecDeque<BinaryHttpResponse>,
    calls: Vec<FakeCall>,
}

#[derive(Clone, Debug)]
pub(super) struct FakeCall {
    pub(super) method: HttpMethod,
    pub(super) url: Url,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) body: Value,
    pub(super) max_response_bytes: Option<usize>,
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
