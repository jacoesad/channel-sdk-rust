use std::fmt;
use std::future::Future;
use std::time::Instant;

use super::ChannelEvent;
use crate::Result;
use crate::lark_openapi::{
    WebSocketConnection, WebSocketEvent, WebSocketEventAck, WebSocketEventFrame,
};

pub trait EventConnection {
    fn next_websocket_event(
        &mut self,
    ) -> impl Future<Output = Result<Option<(WebSocketEventFrame, WebSocketEvent)>>> + Send;

    fn ack_websocket_event(
        &mut self,
        frame: &WebSocketEventFrame,
        ack: WebSocketEventAck,
    ) -> impl Future<Output = Result<()>> + Send;
}

impl EventConnection for WebSocketConnection {
    async fn next_websocket_event(
        &mut self,
    ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>> {
        self.next_event().await
    }

    async fn ack_websocket_event(
        &mut self,
        frame: &WebSocketEventFrame,
        ack: WebSocketEventAck,
    ) -> Result<()> {
        self.ack_event(frame, ack).await
    }
}

pub struct EventConsumer<C> {
    connection: C,
}

impl<C> EventConsumer<C> {
    pub fn new(connection: C) -> Self {
        Self { connection }
    }

    pub fn connection(&self) -> &C {
        &self.connection
    }

    pub fn connection_mut(&mut self) -> &mut C {
        &mut self.connection
    }

    pub fn into_inner(self) -> C {
        self.connection
    }
}

impl<C> EventConsumer<C>
where
    C: EventConnection,
{
    pub async fn next_event(&mut self) -> Result<Option<ReceivedEvent>> {
        let Some((frame, event)) = self.connection.next_websocket_event().await? else {
            return Ok(None);
        };
        let channel_event = match parse_channel_event_or_ack_parse_error(
            &mut self.connection,
            &frame,
            &event,
            WebSocketEventAck::internal_server_error,
        )
        .await
        {
            Ok(event) => event,
            Err(error) => return Err(error),
        };
        Ok(Some(ReceivedEvent::from_parsed_websocket_event(
            frame,
            event,
            channel_event,
        )))
    }

    pub async fn ack_event(&mut self, event: &ReceivedEvent, ack: WebSocketEventAck) -> Result<()> {
        self.connection.ack_websocket_event(&event.frame, ack).await
    }

    pub async fn handle_next_event<H, F>(&mut self, handler: H) -> Result<bool>
    where
        H: FnOnce(ReceivedEvent) -> F,
        F: Future<Output = Result<WebSocketEventAck>> + Send,
    {
        let Some((frame, event)) = self.connection.next_websocket_event().await? else {
            return Ok(false);
        };

        let started = Instant::now();
        let channel_event =
            parse_channel_event_or_ack_parse_error(&mut self.connection, &frame, &event, || {
                WebSocketEventAck::internal_server_error().with_biz_rt(elapsed_millis(started))
            })
            .await?;
        let event = ReceivedEvent::from_parsed_websocket_event(frame.clone(), event, channel_event);
        let ack = handler(event).await?;
        let ack = if ack.biz_rt().is_none() {
            ack.with_biz_rt(elapsed_millis(started))
        } else {
            ack
        };
        self.connection.ack_websocket_event(&frame, ack).await?;
        Ok(true)
    }
}

pub(super) async fn parse_channel_event_or_ack_parse_error<C>(
    connection: &mut C,
    frame: &WebSocketEventFrame,
    event: &WebSocketEvent,
    parse_error_ack: impl FnOnce() -> WebSocketEventAck,
) -> Result<ChannelEvent>
where
    C: EventConnection,
{
    match ChannelEvent::parse_lark_payload(event.payload()) {
        Ok(channel_event) => Ok(channel_event),
        Err(error) => {
            connection
                .ack_websocket_event(frame, parse_error_ack())
                .await?;
            Err(error)
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct ReceivedEvent {
    frame: WebSocketEventFrame,
    pub event: ChannelEvent,
    pub message_id: String,
    pub trace_id: String,
    pub sum: u32,
    pub seq: u32,
    pub payload_len: usize,
    pub payload_encoding: Option<String>,
    pub payload_type: Option<String>,
    pub log_id_new: Option<String>,
}

impl fmt::Debug for ReceivedEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReceivedEvent")
            .field("event", &self.event)
            .field("message_id", &self.message_id)
            .field("trace_id", &self.trace_id)
            .field("sum", &self.sum)
            .field("seq", &self.seq)
            .field("payload_len", &self.payload_len)
            .field("payload_encoding", &self.payload_encoding)
            .field("payload_type", &self.payload_type)
            .field("log_id_new", &self.log_id_new)
            .finish()
    }
}

impl ReceivedEvent {
    pub(super) fn from_parsed_websocket_event(
        frame: WebSocketEventFrame,
        event: WebSocketEvent,
        channel_event: ChannelEvent,
    ) -> Self {
        Self {
            frame,
            event: channel_event,
            message_id: event.message_id().to_owned(),
            trace_id: event.trace_id().to_owned(),
            sum: event.sum(),
            seq: event.seq(),
            payload_len: event.payload().len(),
            payload_encoding: event.payload_encoding().map(str::to_owned),
            payload_type: event.payload_type().map(str::to_owned),
            log_id_new: event.log_id_new().map(str::to_owned),
        }
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use serde_json::json;

    use super::*;
    use crate::Error;
    use crate::lark_openapi::{WebSocketFrame, WebSocketFrameMethod, WebSocketHeader};

    #[derive(Default)]
    struct FakeConnection {
        events: VecDeque<(WebSocketEventFrame, WebSocketEvent)>,
        acks: Vec<WebSocketEventAck>,
    }

    impl EventConnection for FakeConnection {
        async fn next_websocket_event(
            &mut self,
        ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>> {
            Ok(self.events.pop_front())
        }

        async fn ack_websocket_event(
            &mut self,
            _frame: &WebSocketEventFrame,
            ack: WebSocketEventAck,
        ) -> Result<()> {
            self.acks.push(ack);
            Ok(())
        }
    }

    #[tokio::test]
    async fn next_event_parses_channel_event_and_metadata() {
        let mut consumer = EventConsumer::new(fake_connection_with_payload(message_payload()));

        let received = consumer
            .next_event()
            .await
            .expect("receive")
            .expect("event");

        assert_eq!(received.message_id, "om_ws");
        assert_eq!(received.trace_id, "trace_1");
        assert_eq!(received.sum, 1);
        assert_eq!(received.seq, 1);
        assert!(matches!(received.event, ChannelEvent::Message(_)));
        assert!(consumer.connection().acks.is_empty());
    }

    #[tokio::test]
    async fn next_event_acks_parse_errors_as_internal_server_error() {
        let mut consumer = EventConsumer::new(fake_connection_with_payload(b"not json".to_vec()));

        let error = consumer.next_event().await.expect_err("parse error");

        assert!(matches!(error, Error::Serde(_)));
        assert_eq!(consumer.connection().acks.len(), 1);
        assert_eq!(consumer.connection().acks[0].code(), 500);
        assert_eq!(consumer.connection().acks[0].biz_rt(), None);
    }

    #[tokio::test]
    async fn handle_next_event_acks_handler_result_with_elapsed_biz_rt() {
        let mut consumer = EventConsumer::new(fake_connection_with_payload(message_payload()));

        let handled = consumer
            .handle_next_event(|received| async move {
                assert!(matches!(received.event, ChannelEvent::Message(_)));
                Ok(WebSocketEventAck::ok())
            })
            .await
            .expect("handled");

        assert!(handled);
        assert_eq!(consumer.connection().acks.len(), 1);
        assert_eq!(consumer.connection().acks[0].code(), 200);
        assert!(consumer.connection().acks[0].biz_rt().is_some());
    }

    #[tokio::test]
    async fn handle_next_event_preserves_handler_biz_rt() {
        let mut consumer = EventConsumer::new(fake_connection_with_payload(message_payload()));

        let handled = consumer
            .handle_next_event(|_| async { Ok(WebSocketEventAck::ok().with_biz_rt(123)) })
            .await
            .expect("handled");

        assert!(handled);
        assert_eq!(consumer.connection().acks.len(), 1);
        assert_eq!(consumer.connection().acks[0].biz_rt(), Some(123));
    }

    #[tokio::test]
    async fn handle_next_event_does_not_ack_handler_errors() {
        let mut consumer = EventConsumer::new(fake_connection_with_payload(message_payload()));

        let error = consumer
            .handle_next_event(|_| async { Err(Error::Validation("handler failed".to_owned())) })
            .await
            .expect_err("handler error");

        assert!(matches!(error, Error::Validation(message) if message == "handler failed"));
        assert!(consumer.connection().acks.is_empty());
    }

    #[tokio::test]
    async fn handle_next_event_returns_false_at_end_of_stream() {
        let mut consumer = EventConsumer::new(FakeConnection::default());

        let handled = consumer
            .handle_next_event(|_| async { Ok(WebSocketEventAck::ok()) })
            .await
            .expect("handled");

        assert!(!handled);
        assert!(consumer.connection().acks.is_empty());
    }

    #[tokio::test]
    async fn handle_next_event_acks_parse_errors_as_internal_server_error() {
        let mut consumer = EventConsumer::new(fake_connection_with_payload(b"not json".to_vec()));

        let error = consumer
            .handle_next_event(|_| async { Ok(WebSocketEventAck::ok()) })
            .await
            .expect_err("parse error");

        assert!(matches!(error, Error::Serde(_)));
        assert_eq!(consumer.connection().acks.len(), 1);
        assert_eq!(consumer.connection().acks[0].code(), 500);
        assert!(consumer.connection().acks[0].biz_rt().is_some());
    }

    fn fake_connection_with_payload(payload: Vec<u8>) -> FakeConnection {
        let frame = WebSocketFrame {
            seq_id: 1,
            log_id: 2,
            service: 42,
            method: WebSocketFrameMethod::Data as i32,
            headers: vec![
                WebSocketHeader::new("type", "event"),
                WebSocketHeader::new("message_id", "om_ws"),
                WebSocketHeader::new("trace_id", "trace_1"),
                WebSocketHeader::new("sum", "1"),
                WebSocketHeader::new("seq", "1"),
            ],
            payload_encoding: None,
            payload_type: None,
            payload: Some(payload),
            log_id_new: None,
        };
        let (frame, event) = frame.into_event().expect("event frame").expect("event");
        FakeConnection {
            events: VecDeque::from([(frame, event)]),
            acks: Vec::new(),
        }
    }

    fn message_payload() -> Vec<u8> {
        json!({
            "schema": "2.0",
            "header": {
                "event_id": "event_1",
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
                    "message_id": "om_1",
                    "chat_id": "oc_1",
                    "chat_type": "p2p",
                    "message_type": "text",
                    "content": "{\"text\":\"hello\"}"
                }
            }
        })
        .to_string()
        .into_bytes()
    }
}
