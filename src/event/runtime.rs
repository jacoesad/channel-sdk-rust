use std::future::Future;
use std::time::{Duration, Instant};

use tokio::time::Instant as TokioInstant;

use super::consumer::{
    EventConnection, EventConnectionItem, ReceivedEvent, parse_channel_event_or_ack_parse_error,
};
use super::reassembly::{EventPacketReassembler, EventPacketReassemblyOptions};
use crate::lark_openapi::{WebSocketEvent, WebSocketEventAck, WebSocketEventFrame};
use crate::{Error, Result};

pub(super) fn is_reconnectable(error: &Error) -> bool {
    matches!(error, Error::Transport(_))
}

pub(super) enum EventRuntimeDispatchOutcome {
    Handled,
    ReconnectableError(Error),
}

pub(super) struct EventRuntimeDispatcher;

impl EventRuntimeDispatcher {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) async fn dispatch_event<C, H, F>(
        &mut self,
        connection: &mut C,
        handler: &mut H,
        frame: WebSocketEventFrame,
        event: WebSocketEvent,
    ) -> Result<EventRuntimeDispatchOutcome>
    where
        C: EventConnection,
        H: FnMut(ReceivedEvent) -> F,
        F: Future<Output = Result<WebSocketEventAck>> + Send,
    {
        let started = Instant::now();
        let channel_event =
            match parse_channel_event_or_ack_parse_error(connection, &frame, &event, || {
                WebSocketEventAck::internal_server_error().with_biz_rt(elapsed_millis(started))
            })
            .await
            {
                Ok(channel_event) => channel_event,
                Err(error) if is_reconnectable(&error) => {
                    return Ok(EventRuntimeDispatchOutcome::ReconnectableError(error));
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
                        Ok(EventRuntimeDispatchOutcome::ReconnectableError(ack_error))
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
                Ok(EventRuntimeDispatchOutcome::ReconnectableError(error))
            } else {
                Err(error)
            };
        }
        Ok(EventRuntimeDispatchOutcome::Handled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EventRuntimeReceiveOptions {
    heartbeat_timeout: Option<Duration>,
    reassembly_options: EventPacketReassemblyOptions,
}

impl EventRuntimeReceiveOptions {
    pub(super) fn new(
        heartbeat_timeout: Option<Duration>,
        reassembly_options: EventPacketReassemblyOptions,
    ) -> Self {
        Self {
            heartbeat_timeout,
            reassembly_options,
        }
    }
}

pub(super) struct EventRuntimeReceiver {
    heartbeat: HeartbeatSchedule,
    heartbeat_timeout: Option<Duration>,
    reassembler: EventPacketReassembler,
}

impl EventRuntimeReceiver {
    pub(super) fn new<C>(connection: &C, options: EventRuntimeReceiveOptions) -> Self
    where
        C: EventConnection,
    {
        Self {
            heartbeat: HeartbeatSchedule::new(connection.heartbeat_interval()),
            heartbeat_timeout: options.heartbeat_timeout,
            reassembler: EventPacketReassembler::new(options.reassembly_options),
        }
    }

    pub(super) async fn next_event<C>(
        &mut self,
        connection: &mut C,
    ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>>
    where
        C: EventConnection + Send,
    {
        loop {
            let Some((frame, event)) = self.next_packet(connection).await? else {
                return Ok(None);
            };
            match self.reassembler.push(frame.clone(), event) {
                Ok(Some(event)) => return Ok(Some(event)),
                Ok(None) => {}
                Err(error) => {
                    connection
                        .ack_websocket_event(&frame, WebSocketEventAck::internal_server_error())
                        .await?;
                    return Err(error);
                }
            }
        }
    }

    async fn next_packet<C>(
        &mut self,
        connection: &mut C,
    ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>>
    where
        C: EventConnection + Send,
    {
        loop {
            self.heartbeat
                .refresh_interval(connection.heartbeat_interval());

            let Some(deadline) = self.heartbeat.next_deadline() else {
                return match connection.next_websocket_item().await? {
                    EventConnectionItem::Event(frame, event) => {
                        self.heartbeat
                            .mark_activity(connection.heartbeat_interval());
                        Ok(Some((frame, *event)))
                    }
                    EventConnectionItem::Activity => {
                        self.heartbeat
                            .mark_activity(connection.heartbeat_interval());
                        continue;
                    }
                    EventConnectionItem::Closed => Ok(None),
                };
            };

            match tokio::time::timeout_at(deadline.instant(), connection.next_websocket_item())
                .await
            {
                Ok(item) => match item? {
                    EventConnectionItem::Event(frame, event) => {
                        self.heartbeat
                            .mark_activity(connection.heartbeat_interval());
                        return Ok(Some((frame, *event)));
                    }
                    EventConnectionItem::Activity => {
                        self.heartbeat
                            .mark_activity(connection.heartbeat_interval());
                    }
                    EventConnectionItem::Closed => return Ok(None),
                },
                Err(_) if matches!(deadline, HeartbeatDeadline::Liveness(_)) => {
                    return Err(Error::Transport("websocket heartbeat timed out".to_owned()));
                }
                Err(_) => {
                    connection.send_heartbeat().await?;
                    self.heartbeat.mark_heartbeat_sent_with_timeout(
                        connection.heartbeat_interval(),
                        self.heartbeat_timeout,
                    );
                }
            }
        }
    }
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

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}
