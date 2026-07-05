use std::future::Future;
use std::time::Duration;

use tokio::time::Instant as TokioInstant;

use crate::lark_openapi::{
    OpenApiClient, OpenApiTransport, TokioTungsteniteWebSocketTransport, WebSocketClientConfig,
    WebSocketConnection, WebSocketEndpoint, WebSocketEventAck,
};
use crate::{Error, Result};

use super::consumer::parse_channel_event_or_ack_parse_error;
use super::{EventConnection, EventConnectionItem, ReceivedEvent};

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

#[derive(Debug, Clone)]
pub struct OpenApiWebSocketEventConnector<T, W> {
    openapi: OpenApiClient<T>,
    websocket: W,
    last_client_config: Option<WebSocketClientConfig>,
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
}

impl Default for EventLoopOptions {
    fn default() -> Self {
        Self {
            reconnect_limit: EventReconnectLimit::Limited(3),
            reconnect_delay: Duration::from_secs(1),
            use_server_reconnect_config: true,
            heartbeat_timeout: None,
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

fn is_reconnectable(error: &Error) -> bool {
    matches!(error, Error::Transport(_))
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
    let mut heartbeat = HeartbeatSchedule::new(connection.heartbeat_interval());

    loop {
        let Some((frame, event)) = (match next_websocket_event_with_heartbeat(
            connection,
            &mut heartbeat,
            options.heartbeat_timeout(),
        )
        .await
        {
            Ok(event) => event,
            Err(error) if is_reconnectable(&error) => {
                return Ok(ConnectionExit::ReconnectableError(error));
            }
            Err(error) => return Err(error),
        }) else {
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
        let ack = match handler(received).await {
            Ok(ack) => ack,
            Err(error) => {
                if let Err(ack_error) = connection
                    .ack_websocket_event(
                        &frame,
                        WebSocketEventAck::internal_server_error()
                            .with_biz_rt(elapsed_millis(started)),
                    )
                    .await
                {
                    return if is_reconnectable(&ack_error) {
                        Ok(ConnectionExit::ReconnectableError(ack_error))
                    } else {
                        Err(ack_error)
                    };
                }
                return Err(error);
            }
        };
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

struct HeartbeatSchedule {
    interval: Option<Duration>,
    deadline: Option<TokioInstant>,
    liveness_deadline: Option<TokioInstant>,
}

impl HeartbeatSchedule {
    fn new(interval: Option<Duration>) -> Self {
        Self {
            interval,
            deadline: interval.map(|interval| TokioInstant::now() + interval),
            liveness_deadline: None,
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

    fn mark_activity(&mut self, interval: Option<Duration>) {
        self.liveness_deadline = None;
        self.refresh_interval(interval);
    }

    fn mark_heartbeat_sent_with_timeout(
        &mut self,
        interval: Option<Duration>,
        heartbeat_timeout: Option<Duration>,
    ) {
        self.mark_heartbeat_sent(interval);
        if self.liveness_deadline.is_none() {
            self.liveness_deadline = heartbeat_timeout.map(|timeout| TokioInstant::now() + timeout);
        }
    }

    fn next_deadline(&self) -> Option<HeartbeatDeadline> {
        if let Some(liveness) = self.liveness_deadline {
            return Some(HeartbeatDeadline::Liveness(liveness));
        }
        self.deadline.map(HeartbeatDeadline::Heartbeat)
    }
}

enum HeartbeatDeadline {
    Heartbeat(TokioInstant),
    Liveness(TokioInstant),
}

impl HeartbeatDeadline {
    fn instant(&self) -> TokioInstant {
        match self {
            Self::Heartbeat(instant) | Self::Liveness(instant) => *instant,
        }
    }
}

async fn next_websocket_event_with_heartbeat<C>(
    connection: &mut C,
    heartbeat: &mut HeartbeatSchedule,
    heartbeat_timeout: Option<Duration>,
) -> Result<
    Option<(
        crate::lark_openapi::WebSocketEventFrame,
        crate::lark_openapi::WebSocketEvent,
    )>,
>
where
    C: EventConnection + Send,
{
    loop {
        heartbeat.refresh_interval(connection.heartbeat_interval());

        let Some(deadline) = heartbeat.next_deadline() else {
            return match connection.next_websocket_item().await? {
                EventConnectionItem::Event(frame, event) => {
                    heartbeat.mark_activity(connection.heartbeat_interval());
                    Ok(Some((frame, *event)))
                }
                EventConnectionItem::Activity => {
                    heartbeat.mark_activity(connection.heartbeat_interval());
                    continue;
                }
                EventConnectionItem::Closed => Ok(None),
            };
        };

        match tokio::time::timeout_at(deadline.instant(), connection.next_websocket_item()).await {
            Ok(item) => match item? {
                EventConnectionItem::Event(frame, event) => {
                    heartbeat.mark_activity(connection.heartbeat_interval());
                    return Ok(Some((frame, *event)));
                }
                EventConnectionItem::Activity => {
                    heartbeat.mark_activity(connection.heartbeat_interval());
                }
                EventConnectionItem::Closed => return Ok(None),
            },
            Err(_) if matches!(deadline, HeartbeatDeadline::Liveness(_)) => {
                return Err(Error::Transport("websocket heartbeat timed out".to_owned()));
            }
            Err(_) => {
                connection.send_heartbeat().await?;
                heartbeat.mark_heartbeat_sent_with_timeout(
                    connection.heartbeat_interval(),
                    heartbeat_timeout,
                );
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
        WebSocketClientConfig, WebSocketEvent, WebSocketEventFrame, WebSocketFrame,
        WebSocketFrameMethod, WebSocketHeader,
    };

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
