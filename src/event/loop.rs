use std::future::Future;
use std::time::Duration;

use tokio::time::Instant as TokioInstant;

use crate::lark_openapi::{
    OpenApiClient, OpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketConnection,
    WebSocketEndpoint, WebSocketEventAck,
};
use crate::{Error, Result};

use super::consumer::parse_channel_event_or_ack_parse_error;
use super::{EventConnection, ReceivedEvent};

pub trait EventStreamConnector {
    type Connection: EventConnection;

    fn connect_event_stream(&mut self) -> impl Future<Output = Result<Self::Connection>> + Send;
}

pub trait WebSocketEndpointConnector {
    type Connection: EventConnection;

    fn connect_endpoint(
        &mut self,
        endpoint: &WebSocketEndpoint,
    ) -> impl Future<Output = Result<Self::Connection>> + Send;
}

impl WebSocketEndpointConnector for TokioTungsteniteWebSocketTransport {
    type Connection = WebSocketConnection;

    async fn connect_endpoint(&mut self, endpoint: &WebSocketEndpoint) -> Result<Self::Connection> {
        self.connect(endpoint).await
    }
}

#[derive(Debug, Clone)]
pub struct OpenApiWebSocketEventConnector<T, W> {
    openapi: OpenApiClient<T>,
    websocket: W,
}

impl<T, W> OpenApiWebSocketEventConnector<T, W> {
    pub fn new(openapi: OpenApiClient<T>, websocket: W) -> Self {
        Self { openapi, websocket }
    }

    pub fn openapi(&self) -> &OpenApiClient<T> {
        &self.openapi
    }

    pub fn websocket(&self) -> &W {
        &self.websocket
    }

    pub fn websocket_mut(&mut self) -> &mut W {
        &mut self.websocket
    }

    pub fn into_inner(self) -> (OpenApiClient<T>, W) {
        (self.openapi, self.websocket)
    }
}

impl<T, W> EventStreamConnector for OpenApiWebSocketEventConnector<T, W>
where
    T: OpenApiTransport,
    W: WebSocketEndpointConnector + Send,
{
    type Connection = W::Connection;

    async fn connect_event_stream(&mut self) -> Result<Self::Connection> {
        let endpoint = self.openapi.websocket_endpoint().await?;
        self.websocket.connect_endpoint(&endpoint).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventLoopOptions {
    max_reconnects: usize,
    reconnect_delay: Duration,
}

impl Default for EventLoopOptions {
    fn default() -> Self {
        Self {
            max_reconnects: 3,
            reconnect_delay: Duration::from_secs(1),
        }
    }
}

impl EventLoopOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_reconnects(&self) -> usize {
        self.max_reconnects
    }

    pub fn reconnect_delay(&self) -> Duration {
        self.reconnect_delay
    }

    pub fn with_max_reconnects(mut self, max_reconnects: usize) -> Self {
        self.max_reconnects = max_reconnects;
        self
    }

    pub fn with_reconnect_delay(mut self, reconnect_delay: Duration) -> Self {
        self.reconnect_delay = reconnect_delay;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLoopExit {
    ReconnectLimitReached,
}

pub struct EventLoop<C> {
    connector: C,
    options: EventLoopOptions,
}

impl<C> EventLoop<C> {
    pub fn new(connector: C) -> Self {
        Self::with_options(connector, EventLoopOptions::default())
    }

    pub fn with_options(connector: C, options: EventLoopOptions) -> Self {
        Self { connector, options }
    }

    pub fn connector(&self) -> &C {
        &self.connector
    }

    pub fn connector_mut(&mut self) -> &mut C {
        &mut self.connector
    }

    pub fn options(&self) -> EventLoopOptions {
        self.options
    }

    pub fn into_inner(self) -> C {
        self.connector
    }
}

impl<C> EventLoop<C>
where
    C: EventStreamConnector,
{
    pub async fn run<H, F>(&mut self, mut handler: H) -> Result<EventLoopExit>
    where
        H: FnMut(ReceivedEvent) -> F,
        F: Future<Output = Result<WebSocketEventAck>> + Send,
    {
        let mut reconnects = 0;

        loop {
            let connection = match self.connector.connect_event_stream().await {
                Ok(connection) => connection,
                Err(error) if is_reconnectable(&error) => {
                    if !self.wait_before_reconnect(&mut reconnects).await {
                        return Err(error);
                    }
                    continue;
                }
                Err(error) => return Err(error),
            };

            let mut connection = connection;
            let exit = run_connection(&mut connection, &mut handler).await?;
            if !self.wait_before_reconnect(&mut reconnects).await {
                return match exit {
                    ConnectionExit::Closed => Ok(EventLoopExit::ReconnectLimitReached),
                    ConnectionExit::ReconnectableError(error) => Err(error),
                };
            }
        }
    }

    async fn wait_before_reconnect(&self, reconnects: &mut usize) -> bool {
        if *reconnects >= self.options.max_reconnects {
            return false;
        }
        *reconnects += 1;
        if !self.options.reconnect_delay.is_zero() {
            tokio::time::sleep(self.options.reconnect_delay).await;
        }
        true
    }
}

fn is_reconnectable(error: &Error) -> bool {
    matches!(error, Error::Transport(_))
}

enum ConnectionExit {
    Closed,
    ReconnectableError(Error),
}

async fn run_connection<C, H, F>(connection: &mut C, handler: &mut H) -> Result<ConnectionExit>
where
    C: EventConnection,
    H: FnMut(ReceivedEvent) -> F,
    F: Future<Output = Result<WebSocketEventAck>> + Send,
{
    let mut heartbeat = HeartbeatSchedule::new(connection.heartbeat_interval());

    loop {
        let Some((frame, event)) =
            (match next_websocket_event_with_heartbeat(connection, &mut heartbeat).await {
                Ok(event) => event,
                Err(error) if is_reconnectable(&error) => {
                    return Ok(ConnectionExit::ReconnectableError(error));
                }
                Err(error) => return Err(error),
            })
        else {
            return Ok(ConnectionExit::Closed);
        };

        let started = std::time::Instant::now();
        let channel_event =
            match parse_channel_event_or_ack_parse_error(connection, &frame, &event, || {
                WebSocketEventAck::internal_server_error().with_biz_rt(elapsed_millis(started))
            })
            .await
            {
                Ok(channel_event) => channel_event,
                Err(error) if is_reconnectable(&error) => {
                    return Ok(ConnectionExit::ReconnectableError(error));
                }
                Err(error) => return Err(error),
            };
        let received =
            ReceivedEvent::from_parsed_websocket_event(frame.clone(), event, channel_event);
        let ack = handler(received).await?;
        let ack = if ack.biz_rt().is_none() {
            ack.with_biz_rt(elapsed_millis(started))
        } else {
            ack
        };
        if let Err(error) = connection.ack_websocket_event(&frame, ack).await {
            return if is_reconnectable(&error) {
                Ok(ConnectionExit::ReconnectableError(error))
            } else {
                Err(error)
            };
        }
    }
}

struct HeartbeatSchedule {
    interval: Option<Duration>,
    deadline: Option<TokioInstant>,
}

impl HeartbeatSchedule {
    fn new(interval: Option<Duration>) -> Self {
        Self {
            interval,
            deadline: interval.map(|interval| TokioInstant::now() + interval),
        }
    }

    fn refresh_interval(&mut self, interval: Option<Duration>) {
        if self.interval == interval {
            return;
        }
        self.interval = interval;
        self.deadline = interval.map(|interval| TokioInstant::now() + interval);
    }

    fn mark_heartbeat_sent(&mut self, interval: Option<Duration>) {
        self.interval = interval;
        self.deadline = interval.map(|interval| TokioInstant::now() + interval);
    }
}

async fn next_websocket_event_with_heartbeat<C>(
    connection: &mut C,
    heartbeat: &mut HeartbeatSchedule,
) -> Result<
    Option<(
        crate::lark_openapi::WebSocketEventFrame,
        crate::lark_openapi::WebSocketEvent,
    )>,
>
where
    C: EventConnection,
{
    loop {
        heartbeat.refresh_interval(connection.heartbeat_interval());

        let Some(deadline) = heartbeat.deadline else {
            let event = connection.next_websocket_event().await;
            heartbeat.refresh_interval(connection.heartbeat_interval());
            return event;
        };

        match tokio::time::timeout_at(deadline, connection.next_websocket_event()).await {
            Ok(event) => {
                heartbeat.refresh_interval(connection.heartbeat_interval());
                return event;
            }
            Err(_) => {
                connection.send_heartbeat().await?;
                heartbeat.mark_heartbeat_sent(connection.heartbeat_interval());
            }
        }
    }
}

fn elapsed_millis(started: std::time::Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;
    use crate::lark_openapi::{
        WebSocketEvent, WebSocketEventFrame, WebSocketFrame, WebSocketFrameMethod, WebSocketHeader,
    };

    #[derive(Default)]
    struct FakeConnector {
        attempts: usize,
        connections: VecDeque<FakeConnection>,
    }

    impl EventStreamConnector for FakeConnector {
        type Connection = FakeConnection;

        async fn connect_event_stream(&mut self) -> Result<Self::Connection> {
            self.attempts += 1;
            self.connections
                .pop_front()
                .ok_or_else(|| Error::Transport("no fake connection available".to_owned()))
        }
    }

    #[derive(Default)]
    struct FakeConnection {
        events: VecDeque<Result<Option<(WebSocketEventFrame, WebSocketEvent)>>>,
        receive_delays: VecDeque<Duration>,
        ack_results: VecDeque<Result<()>>,
        heartbeat_interval: Option<Duration>,
        heartbeat_results: VecDeque<Result<()>>,
        acks: Arc<AtomicUsize>,
        heartbeats: Arc<AtomicUsize>,
    }

    impl EventConnection for FakeConnection {
        async fn next_websocket_event(
            &mut self,
        ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>> {
            if let Some(delay) = self.receive_delays.pop_front() {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }
            self.events.pop_front().unwrap_or(Ok(None))
        }

        async fn ack_websocket_event(
            &mut self,
            _frame: &WebSocketEventFrame,
            _ack: WebSocketEventAck,
        ) -> Result<()> {
            self.acks.fetch_add(1, Ordering::SeqCst);
            self.ack_results.pop_front().unwrap_or(Ok(()))
        }

        fn heartbeat_interval(&self) -> Option<Duration> {
            self.heartbeat_interval
        }

        async fn send_heartbeat(&mut self) -> Result<()> {
            self.heartbeats.fetch_add(1, Ordering::SeqCst);
            self.heartbeat_results.pop_front().unwrap_or(Ok(()))
        }
    }

    #[tokio::test]
    async fn run_reconnects_after_clean_connection_close() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("one")))],
        ));
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let exit = event_loop
            .run({
                let handled = handled.clone();
                move |_| {
                    let handled = handled.clone();
                    async move {
                        handled.fetch_add(1, Ordering::SeqCst);
                        Ok(WebSocketEventAck::ok())
                    }
                }
            })
            .await
            .expect("event loop exits");

        assert_eq!(exit, EventLoopExit::ReconnectLimitReached);
        assert_eq!(event_loop.connector().attempts, 2);
        assert_eq!(handled.load(Ordering::SeqCst), 2);
        assert_eq!(acks.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn run_reconnects_after_transport_receive_error() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Err(Error::Transport("receive failed".to_owned()))],
        ));
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let exit = event_loop
            .run({
                let handled = handled.clone();
                move |_| {
                    let handled = handled.clone();
                    async move {
                        handled.fetch_add(1, Ordering::SeqCst);
                        Ok(WebSocketEventAck::ok())
                    }
                }
            })
            .await
            .expect("event loop exits");

        assert_eq!(exit, EventLoopExit::ReconnectLimitReached);
        assert_eq!(event_loop.connector().attempts, 2);
        assert_eq!(handled.load(Ordering::SeqCst), 1);
        assert_eq!(acks.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_returns_handler_errors_without_ack_or_reconnect() {
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("one")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let error = event_loop
            .run(|_| async { Err(Error::Validation("handler failed".to_owned())) })
            .await
            .expect_err("handler error");

        assert!(matches!(error, Error::Validation(message) if message == "handler failed"));
        assert_eq!(event_loop.connector().attempts, 1);
        assert_eq!(acks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_does_not_reconnect_handler_transport_errors() {
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("one")))],
        ));
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let error = event_loop
            .run(|_| async { Err(Error::Transport("handler transport failed".to_owned())) })
            .await
            .expect_err("handler error");

        assert!(
            matches!(error, Error::Transport(message) if message == "handler transport failed")
        );
        assert_eq!(event_loop.connector().attempts, 1);
        assert_eq!(acks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_returns_receive_error_after_reconnect_limit() {
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Err(Error::Transport("receive failed".to_owned()))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(0)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let error = event_loop
            .run(|_| async { Ok(WebSocketEventAck::ok()) })
            .await
            .expect_err("receive error");

        assert!(matches!(error, Error::Transport(message) if message == "receive failed"));
        assert_eq!(event_loop.connector().attempts, 1);
        assert_eq!(acks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_reconnects_after_parse_error_ack_transport_error() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector
            .connections
            .push_back(fake_connection_with_ack_results(
                acks.clone(),
                vec![Ok(Some(fake_event_with_payload(
                    "bad",
                    b"not json".to_vec(),
                )))],
                vec![Err(Error::Transport("ack failed".to_owned()))],
            ));
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let exit = event_loop
            .run({
                let handled = handled.clone();
                move |_| {
                    let handled = handled.clone();
                    async move {
                        handled.fetch_add(1, Ordering::SeqCst);
                        Ok(WebSocketEventAck::ok())
                    }
                }
            })
            .await
            .expect("event loop exits");

        assert_eq!(exit, EventLoopExit::ReconnectLimitReached);
        assert_eq!(event_loop.connector().attempts, 2);
        assert_eq!(handled.load(Ordering::SeqCst), 1);
        assert_eq!(acks.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn run_sends_heartbeat_while_waiting_for_event() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let heartbeats = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector
            .connections
            .push_back(fake_connection_with_heartbeat(
                acks.clone(),
                heartbeats.clone(),
                vec![Ok(Some(fake_event("one")))],
                vec![Duration::from_millis(20), Duration::ZERO],
                Some(Duration::from_millis(1)),
                Vec::new(),
            ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(0)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let exit = event_loop
            .run({
                let handled = handled.clone();
                move |_| {
                    let handled = handled.clone();
                    async move {
                        handled.fetch_add(1, Ordering::SeqCst);
                        Ok(WebSocketEventAck::ok())
                    }
                }
            })
            .await
            .expect("event loop exits");

        assert_eq!(exit, EventLoopExit::ReconnectLimitReached);
        assert_eq!(handled.load(Ordering::SeqCst), 1);
        assert_eq!(acks.load(Ordering::SeqCst), 1);
        assert_eq!(heartbeats.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_reconnects_after_heartbeat_transport_error() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let heartbeats = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector
            .connections
            .push_back(fake_connection_with_heartbeat(
                acks.clone(),
                heartbeats.clone(),
                vec![Ok(Some(fake_event("one")))],
                vec![Duration::from_millis(20)],
                Some(Duration::from_millis(1)),
                vec![Err(Error::Transport("heartbeat failed".to_owned()))],
            ));
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let exit = event_loop
            .run({
                let handled = handled.clone();
                move |_| {
                    let handled = handled.clone();
                    async move {
                        handled.fetch_add(1, Ordering::SeqCst);
                        Ok(WebSocketEventAck::ok())
                    }
                }
            })
            .await
            .expect("event loop exits");

        assert_eq!(exit, EventLoopExit::ReconnectLimitReached);
        assert_eq!(event_loop.connector().attempts, 2);
        assert_eq!(handled.load(Ordering::SeqCst), 1);
        assert_eq!(acks.load(Ordering::SeqCst), 1);
        assert_eq!(heartbeats.load(Ordering::SeqCst), 1);
    }

    fn fake_connection(
        acks: Arc<AtomicUsize>,
        events: Vec<Result<Option<(WebSocketEventFrame, WebSocketEvent)>>>,
    ) -> FakeConnection {
        fake_connection_with_ack_results(acks, events, Vec::new())
    }

    fn fake_connection_with_ack_results(
        acks: Arc<AtomicUsize>,
        events: Vec<Result<Option<(WebSocketEventFrame, WebSocketEvent)>>>,
        ack_results: Vec<Result<()>>,
    ) -> FakeConnection {
        fake_connection_with_heartbeat(
            acks,
            Arc::new(AtomicUsize::new(0)),
            events,
            Vec::new(),
            None,
            Vec::new(),
        )
        .with_ack_results(ack_results)
    }

    fn fake_connection_with_heartbeat(
        acks: Arc<AtomicUsize>,
        heartbeats: Arc<AtomicUsize>,
        events: Vec<Result<Option<(WebSocketEventFrame, WebSocketEvent)>>>,
        receive_delays: Vec<Duration>,
        heartbeat_interval: Option<Duration>,
        heartbeat_results: Vec<Result<()>>,
    ) -> FakeConnection {
        FakeConnection {
            events: VecDeque::from(events),
            receive_delays: VecDeque::from(receive_delays),
            ack_results: VecDeque::new(),
            heartbeat_interval,
            heartbeat_results: VecDeque::from(heartbeat_results),
            acks,
            heartbeats,
        }
    }

    impl FakeConnection {
        fn with_ack_results(mut self, ack_results: Vec<Result<()>>) -> Self {
            self.ack_results = VecDeque::from(ack_results);
            self
        }
    }

    fn fake_event(text: &str) -> (WebSocketEventFrame, WebSocketEvent) {
        fake_event_with_payload(text, message_payload(text))
    }

    fn fake_event_with_payload(
        text: &str,
        payload: Vec<u8>,
    ) -> (WebSocketEventFrame, WebSocketEvent) {
        let frame = WebSocketFrame {
            seq_id: 1,
            log_id: 2,
            service: 42,
            method: WebSocketFrameMethod::Data as i32,
            headers: vec![
                WebSocketHeader::new("type", "event"),
                WebSocketHeader::new("message_id", format!("om_{text}")),
                WebSocketHeader::new("trace_id", format!("trace_{text}")),
                WebSocketHeader::new("sum", "1"),
                WebSocketHeader::new("seq", "1"),
            ],
            payload_encoding: None,
            payload_type: None,
            payload: Some(payload),
            log_id_new: None,
        };
        frame.into_event().expect("event frame").expect("event")
    }

    fn message_payload(text: &str) -> Vec<u8> {
        json!({
            "schema": "2.0",
            "header": {
                "event_id": format!("event_{text}"),
                "event_type": "im.message.receive_v1"
            },
            "event": {
                "sender": {
                    "sender_id": {
                        "open_id": "ou_sender"
                    },
                    "sender_type": "user"
                },
                "message": {
                    "message_id": format!("om_{text}"),
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "text",
                    "content": format!("{{\"text\":\"{text}\"}}")
                }
            }
        })
        .to_string()
        .into_bytes()
    }
}
