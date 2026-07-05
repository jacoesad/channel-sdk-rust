use std::collections::{BTreeMap, HashMap, VecDeque};

use crate::lark_openapi::{WebSocketEvent, WebSocketEventFrame};
use crate::{Error, Result};

const DEFAULT_MAX_PARTS: u32 = 1024;
const DEFAULT_MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_PENDING_EVENTS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventPacketReassemblyOptions {
    max_parts: u32,
    max_total_bytes: usize,
    max_pending_events: usize,
}

impl Default for EventPacketReassemblyOptions {
    fn default() -> Self {
        Self {
            max_parts: DEFAULT_MAX_PARTS,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_pending_events: DEFAULT_MAX_PENDING_EVENTS,
        }
    }
}

impl EventPacketReassemblyOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_parts(&self) -> u32 {
        self.max_parts
    }

    pub fn max_total_bytes(&self) -> usize {
        self.max_total_bytes
    }

    pub fn max_pending_events(&self) -> usize {
        self.max_pending_events
    }

    pub fn with_max_parts(mut self, max_parts: u32) -> Self {
        self.max_parts = max_parts.max(1);
        self
    }

    pub fn with_max_total_bytes(mut self, max_total_bytes: usize) -> Self {
        self.max_total_bytes = max_total_bytes.max(1);
        self
    }

    pub fn with_max_pending_events(mut self, max_pending_events: usize) -> Self {
        self.max_pending_events = max_pending_events.max(1);
        self
    }
}

#[derive(Default)]
pub(super) struct EventPacketReassembler {
    options: EventPacketReassemblyOptions,
    pending: HashMap<PendingEventKey, PendingEvent>,
    order: VecDeque<PendingEventKey>,
}

impl EventPacketReassembler {
    pub(super) fn new(options: EventPacketReassemblyOptions) -> Self {
        Self {
            options,
            pending: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(super) fn push(
        &mut self,
        frame: WebSocketEventFrame,
        event: WebSocketEvent,
    ) -> Result<Option<(WebSocketEventFrame, WebSocketEvent)>> {
        let sum = event.sum();
        if sum == 0 {
            return Err(Error::Validation(
                "websocket event packet sum must be at least 1".to_owned(),
            ));
        }
        if sum == 1 {
            return Ok(Some((frame, event)));
        }
        if event.seq() >= sum {
            return Err(Error::Validation(format!(
                "websocket event packet seq {} must be less than sum {}",
                event.seq(),
                sum
            )));
        }
        if sum > self.options.max_parts {
            return Err(Error::Validation(format!(
                "websocket event packet sum {sum} exceeds max_parts {}",
                self.options.max_parts
            )));
        }

        let key = PendingEventKey::from_event(&event);
        if !self.pending.contains_key(&key) {
            self.ensure_pending_capacity();
            self.order.push_back(key.clone());
        }
        let pending = self
            .pending
            .entry(key.clone())
            .or_insert_with(|| PendingEvent::new(sum));
        if pending.sum != sum {
            *pending = PendingEvent::new(sum);
        }
        pending.insert(event.seq(), event.payload());
        let total_bytes = pending.total_bytes;
        if total_bytes > self.options.max_total_bytes {
            self.remove_pending(&key);
            return Err(Error::Validation(format!(
                "websocket event packet payload bytes {} exceeds max_total_bytes {}",
                total_bytes, self.options.max_total_bytes
            )));
        }
        if !pending.is_complete() {
            return Ok(None);
        }

        let payload = pending.reassemble();
        self.remove_pending(&key);
        Ok(Some((frame, event.with_payload(payload))))
    }

    fn ensure_pending_capacity(&mut self) {
        while self.pending.len() >= self.options.max_pending_events {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.pending.remove(&oldest);
        }
    }

    fn remove_pending(&mut self, key: &PendingEventKey) {
        self.pending.remove(key);
        self.order.retain(|candidate| candidate != key);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PendingEventKey {
    message_id: String,
    trace_id: String,
}

impl PendingEventKey {
    fn from_event(event: &WebSocketEvent) -> Self {
        Self {
            message_id: event.message_id().to_owned(),
            trace_id: event.trace_id().to_owned(),
        }
    }
}

struct PendingEvent {
    sum: u32,
    parts: BTreeMap<u32, Vec<u8>>,
    total_bytes: usize,
}

impl PendingEvent {
    fn new(sum: u32) -> Self {
        Self {
            sum,
            parts: BTreeMap::new(),
            total_bytes: 0,
        }
    }

    fn insert(&mut self, seq: u32, payload: &[u8]) {
        let payload = payload.to_vec();
        if let Some(previous) = self.parts.insert(seq, payload) {
            self.total_bytes -= previous.len();
        }
        self.total_bytes += self.parts.get(&seq).map_or(0, Vec::len);
    }

    fn is_complete(&self) -> bool {
        self.parts.len() == self.sum as usize
    }

    fn reassemble(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(self.total_bytes);
        for seq in 0..self.sum {
            if let Some(part) = self.parts.get(&seq) {
                payload.extend_from_slice(part);
            }
        }
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lark_openapi::{WebSocketFrame, WebSocketFrameMethod, WebSocketHeader};

    #[test]
    fn reassembler_passes_single_packet_events_through() {
        let mut reassembler = EventPacketReassembler::new(EventPacketReassemblyOptions::default());
        let (frame, event) = packet("om_1", "trace_1", 1, 1, b"payload");

        let (_, event) = reassembler
            .push(frame, event)
            .expect("push")
            .expect("complete");

        assert_eq!(event.payload(), b"payload");
        assert_eq!(event.sum(), 1);
        assert_eq!(event.seq(), 1);
    }

    #[test]
    fn reassembler_combines_split_packets_in_sequence_order() {
        let mut reassembler = EventPacketReassembler::new(EventPacketReassemblyOptions::default());
        let (frame_1, event_1) = packet("om_1", "trace_1", 3, 1, b"middle");
        let (frame_2, event_2) = packet("om_1", "trace_1", 3, 2, b"end");
        let (frame_0, event_0) = packet("om_1", "trace_1", 3, 0, b"start");

        assert!(reassembler.push(frame_1, event_1).expect("push").is_none());
        assert!(reassembler.push(frame_2, event_2).expect("push").is_none());
        let (_, event) = reassembler
            .push(frame_0, event_0)
            .expect("push")
            .expect("complete");

        assert_eq!(event.payload(), b"startmiddleend");
        assert_eq!(event.sum(), 3);
        assert_eq!(event.seq(), 0);
    }

    #[test]
    fn reassembler_replaces_duplicate_packet_without_double_counting_bytes() {
        let mut reassembler = EventPacketReassembler::new(
            EventPacketReassemblyOptions::default().with_max_total_bytes(6),
        );
        let (frame_0, event_0) = packet("om_1", "trace_1", 2, 0, b"bad");
        let (frame_0_retry, event_0_retry) = packet("om_1", "trace_1", 2, 0, b"ok");
        let (frame_1, event_1) = packet("om_1", "trace_1", 2, 1, b"!");

        assert!(reassembler.push(frame_0, event_0).expect("push").is_none());
        assert!(
            reassembler
                .push(frame_0_retry, event_0_retry)
                .expect("push")
                .is_none()
        );
        let (_, event) = reassembler
            .push(frame_1, event_1)
            .expect("push")
            .expect("complete");

        assert_eq!(event.payload(), b"ok!");
    }

    #[test]
    fn reassembler_rejects_invalid_split_sequence() {
        let mut reassembler = EventPacketReassembler::new(EventPacketReassemblyOptions::default());
        let (frame, event) = packet("om_1", "trace_1", 2, 2, b"bad");

        let error = match reassembler.push(frame, event) {
            Ok(_) => panic!("invalid seq should fail"),
            Err(error) => error,
        };

        assert!(matches!(error, Error::Validation(message) if message.contains("less than sum")));
    }

    #[test]
    fn reassembler_rejects_split_packet_limit_overflow() {
        let mut reassembler =
            EventPacketReassembler::new(EventPacketReassemblyOptions::default().with_max_parts(2));
        let (frame, event) = packet("om_1", "trace_1", 3, 0, b"too many");

        let error = match reassembler.push(frame, event) {
            Ok(_) => panic!("too many parts should fail"),
            Err(error) => error,
        };

        assert!(matches!(error, Error::Validation(message) if message.contains("max_parts")));
    }

    #[test]
    fn reassembler_rejects_total_byte_limit_overflow() {
        let mut reassembler = EventPacketReassembler::new(
            EventPacketReassemblyOptions::default().with_max_total_bytes(3),
        );
        let (frame_0, event_0) = packet("om_1", "trace_1", 2, 0, b"aa");
        let (frame_1, event_1) = packet("om_1", "trace_1", 2, 1, b"bb");

        assert!(reassembler.push(frame_0, event_0).expect("push").is_none());
        let error = match reassembler.push(frame_1, event_1) {
            Ok(_) => panic!("too many bytes should fail"),
            Err(error) => error,
        };

        assert!(matches!(error, Error::Validation(message) if message.contains("max_total_bytes")));
    }

    #[test]
    fn reassembler_evicts_oldest_pending_event_when_capacity_is_reached() {
        let mut reassembler = EventPacketReassembler::new(
            EventPacketReassemblyOptions::default().with_max_pending_events(1),
        );
        let (old_frame_0, old_event_0) = packet("om_old", "trace_old", 2, 0, b"old-");
        let (new_frame_0, new_event_0) = packet("om_new", "trace_new", 2, 0, b"new-");
        let (old_frame_1, old_event_1) = packet("om_old", "trace_old", 2, 1, b"done");
        let (old_frame_0_retry, old_event_0_retry) = packet("om_old", "trace_old", 2, 0, b"old-");

        assert!(
            reassembler
                .push(old_frame_0, old_event_0)
                .expect("push")
                .is_none()
        );
        assert!(
            reassembler
                .push(new_frame_0, new_event_0)
                .expect("push")
                .is_none()
        );
        assert!(
            reassembler
                .push(old_frame_1, old_event_1)
                .expect("push")
                .is_none()
        );
        let (_, event) = reassembler
            .push(old_frame_0_retry, old_event_0_retry)
            .expect("push")
            .expect("complete");

        assert_eq!(event.payload(), b"old-done");
    }

    fn packet(
        message_id: &str,
        trace_id: &str,
        sum: u32,
        seq: u32,
        payload: &[u8],
    ) -> (WebSocketEventFrame, WebSocketEvent) {
        WebSocketFrame {
            seq_id: 1,
            log_id: 2,
            service: 42,
            method: WebSocketFrameMethod::Data as i32,
            headers: vec![
                WebSocketHeader::new("type", "event"),
                WebSocketHeader::new("message_id", message_id),
                WebSocketHeader::new("trace_id", trace_id),
                WebSocketHeader::new("sum", sum.to_string()),
                WebSocketHeader::new("seq", seq.to_string()),
            ],
            payload_encoding: None,
            payload_type: Some("application/json".to_owned()),
            payload: Some(payload.to_vec()),
            log_id_new: None,
        }
        .into_event()
        .expect("event")
        .expect("event")
    }
}
