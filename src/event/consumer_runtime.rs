use super::ChannelEvent;
use super::consumer::EventConnection;
use super::reassembly::{EventPacketReassembler, EventPacketReassemblyOptions};
use crate::Result;
use crate::lark_openapi::{WebSocketEvent, WebSocketEventAck, WebSocketEventFrame};

pub(super) struct EventConsumerRuntime {
    reassembler: EventPacketReassembler,
}

impl EventConsumerRuntime {
    pub(super) fn new(reassembly_options: EventPacketReassemblyOptions) -> Self {
        Self {
            reassembler: EventPacketReassembler::new(reassembly_options),
        }
    }

    pub(super) async fn next_reassembled_event<C>(
        &mut self,
        connection: &mut C,
    ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>>
    where
        C: EventConnection,
    {
        loop {
            let Some((frame, event)) = connection.next_websocket_event().await? else {
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

    pub(super) async fn parse_or_ack_parse_error<C>(
        &self,
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
}
