use std::fmt;
use std::future::Future;
use std::time::Duration;

use crate::lark_openapi::{
    OpenApiClient, OpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketClientConfig,
    WebSocketConnection, WebSocketEndpoint, WebSocketEventAck,
};
use crate::{Error, Result};

use super::reassembly::EventPacketReassemblyOptions;
use super::runtime::{
    EventRuntimeDispatchOutcome, EventRuntimeDispatcher, EventRuntimeReceiveOptions,
    EventRuntimeReceiver, is_reconnectable,
};
use super::{EventConnection, ReceivedEvent};

pub trait EventStreamConnector {
    type Connection: EventConnection;

    fn connect_event_stream(&mut self) -> impl Future<Output = Result<Self::Connection>> + Send;

    fn websocket_client_config(&self) -> Option<WebSocketClientConfig> {
        None
    }
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

#[derive(Clone)]
pub struct OpenApiWebSocketEventConnector<T, W> {
    openapi: OpenApiClient<T>,
    websocket: W,
    last_client_config: Option<WebSocketClientConfig>,
}

impl<T, W> fmt::Debug for OpenApiWebSocketEventConnector<T, W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenApiWebSocketEventConnector")
            .field("openapi", &self.openapi)
            .field("websocket_type", &std::any::type_name::<W>())
            .field("last_client_config", &self.last_client_config)
            .finish_non_exhaustive()
    }
}

impl<T, W> OpenApiWebSocketEventConnector<T, W> {
    pub fn new(openapi: OpenApiClient<T>, websocket: W) -> Self {
        Self {
            openapi,
            websocket,
            last_client_config: None,
        }
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
        self.last_client_config = None;
        let endpoint = self.openapi.websocket_endpoint().await?;
        self.last_client_config = endpoint.client_config().copied();
        self.websocket.connect_endpoint(&endpoint).await
    }

    fn websocket_client_config(&self) -> Option<WebSocketClientConfig> {
        self.last_client_config
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventReconnectLimit {
    Limited(usize),
    Unlimited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventLoopOptions {
    reconnect_limit: EventReconnectLimit,
    reconnect_delay: Duration,
    use_server_reconnect_config: bool,
    heartbeat_timeout: Option<Duration>,
    reassembly_options: EventPacketReassemblyOptions,
}

impl Default for EventLoopOptions {
    fn default() -> Self {
        Self {
            reconnect_limit: EventReconnectLimit::Limited(3),
            reconnect_delay: Duration::from_secs(1),
            use_server_reconnect_config: true,
            heartbeat_timeout: None,
            reassembly_options: EventPacketReassemblyOptions::default(),
        }
    }
}

impl EventLoopOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_reconnects(&self) -> usize {
        match self.reconnect_limit {
            EventReconnectLimit::Limited(max_reconnects) => max_reconnects,
            EventReconnectLimit::Unlimited => usize::MAX,
        }
    }

    pub fn reconnect_limit(&self) -> EventReconnectLimit {
        self.reconnect_limit
    }

    pub fn reconnect_delay(&self) -> Duration {
        self.reconnect_delay
    }

    pub fn use_server_reconnect_config(&self) -> bool {
        self.use_server_reconnect_config
    }

    pub fn heartbeat_timeout(&self) -> Option<Duration> {
        self.heartbeat_timeout
    }

    pub fn reassembly_options(&self) -> EventPacketReassemblyOptions {
        self.reassembly_options
    }

    pub fn with_max_reconnects(mut self, max_reconnects: usize) -> Self {
        self.reconnect_limit = EventReconnectLimit::Limited(max_reconnects);
        self.use_server_reconnect_config = false;
        self
    }

    pub fn with_unlimited_reconnects(mut self) -> Self {
        self.reconnect_limit = EventReconnectLimit::Unlimited;
        self.use_server_reconnect_config = false;
        self
    }

    pub fn with_reconnect_delay(mut self, reconnect_delay: Duration) -> Self {
        self.reconnect_delay = reconnect_delay;
        self.use_server_reconnect_config = false;
        self
    }

    pub fn with_server_reconnect_config(mut self, enabled: bool) -> Self {
        self.use_server_reconnect_config = enabled;
        self
    }

    pub fn with_heartbeat_timeout(mut self, heartbeat_timeout: Option<Duration>) -> Self {
        self.heartbeat_timeout = heartbeat_timeout.filter(|timeout| !timeout.is_zero());
        self
    }

    pub fn with_reassembly_options(
        mut self,
        reassembly_options: EventPacketReassemblyOptions,
    ) -> Self {
        self.reassembly_options = reassembly_options;
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
    C::Connection: Send,
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
                    let fallback_policy = EffectiveReconnectPolicy::from_options(
                        self.options,
                        self.connector.websocket_client_config(),
                    );
                    if !self
                        .wait_before_reconnect(&mut reconnects, fallback_policy)
                        .await
                    {
                        return Err(error);
                    }
                    continue;
                }
                Err(error) => return Err(error),
            };

            let mut connection = connection;
            let exit = run_connection(&mut connection, &mut handler, self.options).await?;
            let reconnect_policy = EffectiveReconnectPolicy::from_options(
                self.options,
                connection.websocket_client_config(),
            );
            if !self
                .wait_before_reconnect(&mut reconnects, reconnect_policy)
                .await
            {
                return match exit {
                    ConnectionExit::Closed => Ok(EventLoopExit::ReconnectLimitReached),
                    ConnectionExit::ReconnectableError(error) => Err(error),
                };
            }
        }
    }

    async fn wait_before_reconnect(
        &self,
        reconnects: &mut usize,
        policy: EffectiveReconnectPolicy,
    ) -> bool {
        if !policy.reconnect_limit.allows(*reconnects) {
            return false;
        }
        let delay = if *reconnects == 0 {
            policy.initial_delay.unwrap_or(policy.reconnect_delay)
        } else {
            policy.reconnect_delay
        };
        *reconnects += 1;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        true
    }
}

enum ConnectionExit {
    Closed,
    ReconnectableError(Error),
}

async fn run_connection<C, H, F>(
    connection: &mut C,
    handler: &mut H,
    options: EventLoopOptions,
) -> Result<ConnectionExit>
where
    C: EventConnection + Send,
    H: FnMut(ReceivedEvent) -> F,
    F: Future<Output = Result<WebSocketEventAck>> + Send,
{
    let receive_options =
        EventRuntimeReceiveOptions::new(options.heartbeat_timeout(), options.reassembly_options());
    let mut receiver = EventRuntimeReceiver::new(connection, receive_options);
    let mut dispatcher = EventRuntimeDispatcher::new();

    loop {
        let Some((frame, event)) = (match receiver.next_event(connection).await {
            Ok(event) => event,
            Err(error) if is_reconnectable(&error) => {
                return Ok(ConnectionExit::ReconnectableError(error));
            }
            Err(error) => return Err(error),
        }) else {
            return Ok(ConnectionExit::Closed);
        };

        match dispatcher
            .dispatch_event(connection, handler, frame, event)
            .await?
        {
            EventRuntimeDispatchOutcome::Handled => {}
            EventRuntimeDispatchOutcome::ReconnectableError(error) => {
                return Ok(ConnectionExit::ReconnectableError(error));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EffectiveReconnectPolicy {
    reconnect_limit: EventReconnectLimit,
    reconnect_delay: Duration,
    initial_delay: Option<Duration>,
}

impl EffectiveReconnectPolicy {
    fn from_options(
        options: EventLoopOptions,
        client_config: Option<crate::lark_openapi::WebSocketClientConfig>,
    ) -> Self {
        let mut policy = Self {
            reconnect_limit: options.reconnect_limit(),
            reconnect_delay: options.reconnect_delay(),
            initial_delay: None,
        };
        if !options.use_server_reconnect_config() {
            return policy;
        }
        let Some(config) = client_config else {
            return policy;
        };
        if let Some(limit) = reconnect_limit_from_count(config.reconnect_count) {
            policy.reconnect_limit = limit;
        }
        if let Some(interval) = config.reconnect_interval() {
            policy.reconnect_delay = interval;
        }
        policy.initial_delay = config.reconnect_nonce().map(reconnect_jitter);
        policy
    }
}

impl EventReconnectLimit {
    fn allows(self, attempted_reconnects: usize) -> bool {
        match self {
            Self::Limited(limit) => attempted_reconnects < limit,
            Self::Unlimited => true,
        }
    }
}

fn reconnect_limit_from_count(count: Option<i32>) -> Option<EventReconnectLimit> {
    match count {
        Some(-1) => Some(EventReconnectLimit::Unlimited),
        Some(count) if count >= 0 => Some(EventReconnectLimit::Limited(count as usize)),
        _ => None,
    }
}

fn reconnect_jitter(max: Duration) -> Duration {
    if max.is_zero() {
        return Duration::ZERO;
    }
    let max_millis = max.as_millis().min(u64::MAX as u128) as u64;
    if max_millis == 0 {
        return Duration::ZERO;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Duration::from_millis(u64::from(now.subsec_nanos()) % max_millis.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;
    use crate::ChannelConfig;
    use crate::event::ChannelEvent;
    use crate::lark_openapi::test_support::FakeTransport;
    use crate::lark_openapi::{
        WebSocketClientConfig, WebSocketEvent, WebSocketEventFrame, WebSocketFrame,
        WebSocketFrameMethod, WebSocketHeader,
    };

    #[test]
    fn openapi_websocket_connector_debug_does_not_format_connector_state() {
        let openapi = OpenApiClient::new(
            ChannelConfig::new("cli_test", "app-secret"),
            FakeTransport::new(Vec::new()),
        );
        let connector = OpenApiWebSocketEventConnector::new(openapi, "websocket-secret".to_owned());

        let debug = format!("{connector:?}");

        assert!(debug.contains("websocket_type"));
        assert!(!debug.contains("app-secret"));
        assert!(!debug.contains("websocket-secret"));
    }

    #[derive(Default)]
    struct FakeConnector {
        attempts: usize,
        connections: VecDeque<FakeConnection>,
        client_config: Option<WebSocketClientConfig>,
    }

    impl EventStreamConnector for FakeConnector {
        type Connection = FakeConnection;

        async fn connect_event_stream(&mut self) -> Result<Self::Connection> {
            self.attempts += 1;
            self.connections
                .pop_front()
                .ok_or_else(|| Error::Transport("no fake connection available".to_owned()))
        }

        fn websocket_client_config(&self) -> Option<WebSocketClientConfig> {
            self.client_config
        }
    }

    #[derive(Default)]
    struct FakeConnection {
        events: VecDeque<Result<Option<(WebSocketEventFrame, WebSocketEvent)>>>,
        receive_delays: VecDeque<Duration>,
        pending_receives: usize,
        ack_results: VecDeque<Result<()>>,
        heartbeat_interval: Option<Duration>,
        heartbeat_results: VecDeque<Result<()>>,
        client_config: Option<WebSocketClientConfig>,
        acks: Arc<AtomicUsize>,
        heartbeats: Arc<AtomicUsize>,
    }

    impl EventConnection for FakeConnection {
        async fn next_websocket_event(
            &mut self,
        ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>> {
            if self.pending_receives > 0 {
                self.pending_receives -= 1;
                std::future::pending::<()>().await;
                unreachable!("pending future should not complete");
            }
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

        fn websocket_client_config(&self) -> Option<WebSocketClientConfig> {
            self.client_config
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
    async fn run_reassembles_split_packets_before_handler_and_ack() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let payload = message_payload("split");
        let payload_len = payload.len();
        let split_at = payload.len() / 2;
        let mut connector = FakeConnector::default();
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![
                Ok(Some(fake_event_packet(
                    "split",
                    2,
                    1,
                    payload[split_at..].to_vec(),
                ))),
                Ok(Some(fake_event_packet(
                    "split",
                    2,
                    0,
                    payload[..split_at].to_vec(),
                ))),
            ],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(0)
            .with_reconnect_delay(Duration::ZERO);
        let mut event_loop = EventLoop::with_options(connector, options);

        let exit = event_loop
            .run({
                let handled = handled.clone();
                move |event| {
                    let handled = handled.clone();
                    async move {
                        handled.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(event.payload_len, payload_len);
                        assert_eq!(event.sum, 2);
                        assert_eq!(event.seq, 0);
                        match event.event {
                            ChannelEvent::Message(message) => {
                                assert_eq!(message.message_id, "om_split");
                                assert_eq!(message.text, "split");
                            }
                            other => panic!("expected message event, got {other:?}"),
                        }
                        Ok(WebSocketEventAck::ok())
                    }
                }
            })
            .await
            .expect("event loop exits");

        assert_eq!(exit, EventLoopExit::ReconnectLimitReached);
        assert_eq!(handled.load(Ordering::SeqCst), 1);
        assert_eq!(acks.load(Ordering::SeqCst), 1);
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
    async fn run_acks_handler_errors_as_internal_server_error_without_reconnect() {
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
        assert_eq!(acks.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_acks_handler_transport_errors_without_reconnect() {
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
        assert_eq!(acks.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_uses_server_reconnect_count_when_enabled() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector
            .connections
            .push_back(fake_connection(acks.clone(), vec![]).with_client_config(
                WebSocketClientConfig {
                    reconnect_count: Some(0),
                    reconnect_interval: Some(1),
                    reconnect_nonce: Some(1),
                    ping_interval: None,
                },
            ));
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let mut event_loop = EventLoop::new(connector);
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
        assert_eq!(event_loop.connector().attempts, 1);
        assert_eq!(handled.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_uses_server_reconnect_count_after_connect_error() {
        let connector = FakeConnector {
            client_config: Some(WebSocketClientConfig {
                reconnect_count: Some(0),
                reconnect_interval: Some(1),
                reconnect_nonce: Some(1),
                ping_interval: None,
            }),
            ..FakeConnector::default()
        };
        let mut event_loop = EventLoop::new(connector);

        let error = event_loop
            .run(|_| async { Ok(WebSocketEventAck::ok()) })
            .await
            .expect_err("connect error");

        assert!(
            matches!(error, Error::Transport(message) if message == "no fake connection available")
        );
        assert_eq!(event_loop.connector().attempts, 1);
    }

    #[tokio::test]
    async fn explicit_reconnect_options_override_server_reconnect_count() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector
            .connections
            .push_back(fake_connection(acks.clone(), vec![]).with_client_config(
                WebSocketClientConfig {
                    reconnect_count: Some(0),
                    reconnect_interval: Some(1),
                    reconnect_nonce: Some(1),
                    ping_interval: None,
                },
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
                Some(Duration::from_millis(10)),
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
    async fn run_does_not_send_heartbeat_while_handler_is_running() {
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
                Vec::new(),
                Some(Duration::from_millis(5)),
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
                        tokio::time::sleep(Duration::from_millis(15)).await;
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
        assert_eq!(heartbeats.load(Ordering::SeqCst), 0);
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
                Some(Duration::from_millis(10)),
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

    #[tokio::test]
    async fn run_reconnects_after_heartbeat_liveness_timeout() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let heartbeats = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector.connections.push_back(
            fake_connection_with_heartbeat(
                acks.clone(),
                heartbeats.clone(),
                vec![Ok(Some(fake_event("one")))],
                Vec::new(),
                Some(Duration::from_millis(10)),
                Vec::new(),
            )
            .with_pending_receives(2),
        );
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO)
            .with_heartbeat_timeout(Some(Duration::from_millis(1)));
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

    #[tokio::test]
    async fn heartbeat_liveness_timeout_is_not_postponed_by_followup_heartbeats() {
        let handled = Arc::new(AtomicUsize::new(0));
        let acks = Arc::new(AtomicUsize::new(0));
        let heartbeats = Arc::new(AtomicUsize::new(0));
        let mut connector = FakeConnector::default();
        connector.connections.push_back(
            fake_connection_with_heartbeat(
                acks.clone(),
                heartbeats.clone(),
                vec![Ok(Some(fake_event("one")))],
                Vec::new(),
                Some(Duration::from_millis(1)),
                Vec::new(),
            )
            .with_pending_receives(2),
        );
        connector.connections.push_back(fake_connection(
            acks.clone(),
            vec![Ok(Some(fake_event("two")))],
        ));

        let options = EventLoopOptions::new()
            .with_max_reconnects(1)
            .with_reconnect_delay(Duration::ZERO)
            .with_heartbeat_timeout(Some(Duration::from_millis(20)));
        let mut event_loop = EventLoop::with_options(connector, options);

        let exit = tokio::time::timeout(
            Duration::from_millis(100),
            event_loop.run({
                let handled = handled.clone();
                move |_| {
                    let handled = handled.clone();
                    async move {
                        handled.fetch_add(1, Ordering::SeqCst);
                        Ok(WebSocketEventAck::ok())
                    }
                }
            }),
        )
        .await
        .expect("liveness timeout should not be postponed forever")
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
            pending_receives: 0,
            ack_results: VecDeque::new(),
            heartbeat_interval,
            heartbeat_results: VecDeque::from(heartbeat_results),
            client_config: None,
            acks,
            heartbeats,
        }
    }

    impl FakeConnection {
        fn with_ack_results(mut self, ack_results: Vec<Result<()>>) -> Self {
            self.ack_results = VecDeque::from(ack_results);
            self
        }

        fn with_client_config(mut self, client_config: WebSocketClientConfig) -> Self {
            self.client_config = Some(client_config);
            self
        }

        fn with_pending_receives(mut self, pending_receives: usize) -> Self {
            self.pending_receives = pending_receives;
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
        fake_event_packet(text, 1, 1, payload)
    }

    fn fake_event_packet(
        text: &str,
        sum: u32,
        seq: u32,
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
                WebSocketHeader::new("sum", sum.to_string()),
                WebSocketHeader::new("seq", seq.to_string()),
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
