use serde_json::Value;

use super::*;
use crate::lark_openapi::{HttpMethod, HttpResponse};
use crate::message::streaming::test_support::{
    FakeResponse, FakeTransport, assert_pending_once, block_on, card_response, message_response,
    ok_response, token_response,
};
use crate::{ChannelConfig, Recipient};

#[test]
fn rolls_long_content_onto_follow_up_cards_without_losing_source() {
    let transport = FakeTransport::http(vec![
        token_response(),
        card_response("7355372766134157501"),
        message_response("om_page_1"),
        ok_response(),
        ok_response(),
        card_response("7355372766134157502"),
        message_response("om_page_2"),
        ok_response(),
        ok_response(),
        ok_response(),
    ]);
    let sender = sender(&transport);
    let source = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda";
    let mut stream = block_on(
        sender
            .markdown_stream_reply(MessageId("om_parent".to_owned()))
            .reply_in_thread(true)
            .continuation_max_page_chars(40)
            .start_continuing(),
    )
    .expect("continuing stream")
    .throttle(Duration::from_secs(60));

    block_on(stream.append(source)).expect("rolled content");
    assert_eq!(stream.content(), Some(source));
    assert_eq!(stream.pages().len(), 2);
    assert_eq!(stream.first_message_id().0, "om_page_1");
    assert_eq!(stream.current_message_id().0, "om_page_2");

    let calls = transport.calls();
    let first_content = calls[3].body["content"].as_str().expect("first content");
    let second_content = calls[7].body["content"].as_str().expect("second content");
    assert_eq!(format!("{first_content}{second_content}"), source);
    assert_eq!(calls[4].method, HttpMethod::Patch);
    assert!(calls[6].body["uuid"].as_str().is_some());
    assert!(calls[2].url.path().ends_with("/messages/om_parent/reply"));
    assert!(calls[6].url.path().ends_with("/messages/om_parent/reply"));
    assert_eq!(calls[2].body["reply_in_thread"], true);
    assert_eq!(calls[6].body["reply_in_thread"], true);
    assert_eq!(calls[3].body["sequence"], 1);
    assert_eq!(calls[4].body["sequence"], 2);
    assert_eq!(calls[7].body["sequence"], 1);
    assert_ne!(calls[3].body["uuid"], calls[7].body["uuid"]);

    block_on(stream.append(" tail")).expect("buffered second-page tail");
    assert_eq!(transport.calls().len(), 8);
    assert!(stream.next_flush_in().is_some_and(|wait| !wait.is_zero()));
    block_on(stream.flush()).expect("flushed second-page tail");
    let calls = transport.calls();
    assert_eq!(calls[8].body["sequence"], 2);
    assert_eq!(
        calls[8].body["content"].as_str(),
        Some(format!("{second_content} tail").as_str())
    );

    let calls_before_rejected_snapshot = calls.len();
    let error = block_on(stream.set_content("rewritten content"))
        .expect_err("finalized source cannot be rewritten");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(transport.calls().len(), calls_before_rejected_snapshot);

    block_on(stream.finish()).expect("finished final page");
    assert!(stream.is_finished());
    assert_eq!(transport.calls().len(), 10);

    block_on(stream.flush()).expect("finished flush is a no-op");
    let error =
        block_on(stream.append("")).expect_err("finished stream rejects even empty append calls");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(transport.calls().len(), 10);
}

#[test]
fn retry_reuses_prepared_follow_up_card_and_message_uuid() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157511")),
        Ok(message_response("om_page_1")),
        Ok(ok_response()),
        Ok(ok_response()),
        Ok(card_response("7355372766134157512")),
        Err(Error::Transport("follow-up response lost".to_owned())),
        Ok(message_response("om_page_2")),
        Ok(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");

    let source = "word ".repeat(12);
    let error = block_on(stream.append(&source)).expect_err("ambiguous follow-up send");
    assert!(matches!(error, Error::Transport(_)));
    assert!(stream.has_pending_operation());
    assert!(!stream.is_recovery_blocked());
    assert_eq!(stream.pages().len(), 1);

    block_on(stream.retry_pending()).expect("recovered follow-up send");
    assert_eq!(stream.pages().len(), 2);
    assert_eq!(stream.content(), Some(source.as_str()));

    let calls = transport.calls();
    assert_eq!(calls.len(), 9);
    assert_eq!(calls[6].body["uuid"], calls[7].body["uuid"]);
    assert_eq!(calls[6].body["content"], calls[7].body["content"]);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.path().ends_with("/cardkit/v1/cards"))
            .count(),
        2
    );
}

#[test]
fn ambiguous_follow_up_card_creation_is_not_repeated() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157513")),
        Ok(message_response("om_page_1")),
        Ok(ok_response()),
        Ok(ok_response()),
        Err(Error::Transport("follow-up card response lost".to_owned())),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");

    let source = "word ".repeat(12);
    let error = block_on(stream.append(&source)).expect_err("ambiguous follow-up create");
    assert!(matches!(error, Error::Transport(_)));
    assert!(!stream.has_pending_operation());
    assert!(stream.is_recovery_blocked());
    assert_eq!(stream.next_flush_in(), None);
    assert_eq!(stream.pages().len(), 1);

    let error = block_on(stream.retry_pending()).expect_err("create cannot be replayed safely");
    assert!(matches!(error, Error::Validation(message) if message.contains("unknown outcome")));
    assert_eq!(transport.calls().len(), 6);
    assert_eq!(stream.content(), Some(source.as_str()));
}

#[test]
fn pending_finish_blocks_content_changes_until_replayed() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157521")),
        Ok(message_response("om_finishing")),
        Ok(ok_response()),
        Err(Error::Transport("finish response lost".to_owned())),
        Ok(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");

    block_on(stream.append("content")).expect("content update");
    let error = block_on(stream.finish()).expect_err("ambiguous finish");
    assert!(matches!(error, Error::Transport(_)));
    let calls_before_append = transport.calls().len();

    let error = block_on(stream.append("")).expect_err("pending finish keeps content immutable");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(transport.calls().len(), calls_before_append);

    block_on(stream.retry_pending()).expect("finish replay");
    assert!(stream.is_finished());
    let calls = transport.calls();
    assert_eq!(calls[4].body["uuid"], calls[5].body["uuid"]);
    assert_eq!(calls[4].body["sequence"], calls[5].body["sequence"]);
}

#[test]
fn cancelled_buffered_finish_retains_finalization_intent() {
    let transport = FakeTransport::scripted(vec![
        FakeResponse::ready(token_response()),
        FakeResponse::ready(card_response("7355372766134157522")),
        FakeResponse::ready(message_response("om_buffered_finish")),
        FakeResponse::ready(ok_response()),
        FakeResponse::pending_once(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .start_continuing(),
    )
    .expect("continuing stream")
    .throttle(Duration::from_secs(60));

    block_on(stream.append("A")).expect("first update");
    block_on(stream.append("B")).expect("buffered tail");
    assert_pending_once(stream.finish());
    assert!(stream.has_pending_operation());
    assert_eq!(stream.next_flush_in(), Some(Duration::ZERO));

    let calls_before_rejected_append = transport.calls().len();
    let error =
        block_on(stream.append("late")).expect_err("finishing continuation must remain immutable");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(transport.calls().len(), calls_before_rejected_append);

    block_on(stream.retry_pending()).expect("resume buffered finish");
    assert!(stream.is_finished());
    assert!(!stream.has_pending_operation());
    let calls = transport.calls();
    assert_eq!(calls[4].body["sequence"], 2);
    assert_eq!(calls[5].body["sequence"], 2);
    assert_eq!(calls[4].body["uuid"], calls[5].body["uuid"]);
    assert_eq!(calls[6].body["sequence"], 3);
}

#[test]
fn retry_replays_current_page_update_before_rollover() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157531")),
        Ok(message_response("om_page_1")),
        Err(Error::Transport("page update response lost".to_owned())),
        Ok(ok_response()),
        Ok(ok_response()),
        Ok(card_response("7355372766134157532")),
        Ok(message_response("om_page_2")),
        Ok(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");

    let error =
        block_on(stream.append("word ".repeat(12))).expect_err("ambiguous current-page update");
    assert!(matches!(error, Error::Transport(_)));
    block_on(stream.retry_pending()).expect("replayed rollover");

    let calls = transport.calls();
    assert_eq!(calls[3].body["sequence"], 1);
    assert_eq!(calls[4].body["sequence"], 1);
    assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
    assert_eq!(calls[5].body["sequence"], 2);
    assert_eq!(calls[8].body["sequence"], 1);
    assert_ne!(calls[3].body["uuid"], calls[8].body["uuid"]);
}

#[test]
fn append_plans_against_the_recovered_rollover_state() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157535")),
        Ok(message_response("om_projected_page_1")),
        Err(Error::Transport("page update response lost".to_owned())),
        Ok(ok_response()),
        Ok(ok_response()),
        Ok(card_response("7355372766134157536")),
        Ok(message_response("om_projected_page_2")),
        Ok(ok_response()),
        Ok(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(34)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");
    let source = "word ".repeat(12);

    let error = block_on(stream.append(&source)).expect_err("ambiguous current-page update");
    assert!(matches!(error, Error::Transport(_)));
    block_on(stream.append("tail")).expect("recover rollover before applying the extension");

    let calls = transport.calls();
    assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
    assert_eq!(calls[8].body["sequence"], 1);
    assert_eq!(calls[9].body["sequence"], 2);
    assert!(
        calls[9].body["content"]
            .as_str()
            .is_some_and(|content| content.ends_with("tail"))
    );
    assert_eq!(stream.pages().len(), 2);
    assert_eq!(stream.content(), Some(format!("{source}tail").as_str()));
    assert!(!stream.has_pending_operation());
}

#[test]
fn http_status_during_rollover_reuses_the_current_page_operation() {
    let transport = FakeTransport::http(vec![
        token_response(),
        card_response("7355372766134157533"),
        message_response("om_page_1"),
        HttpResponse::json(503, Value::Null),
        ok_response(),
        ok_response(),
        card_response("7355372766134157534"),
        message_response("om_page_2"),
        ok_response(),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .start_continuing(),
    )
    .expect("continuing stream");

    let error = block_on(stream.append("word ".repeat(12)))
        .expect_err("HTTP status leaves the rollover update ambiguous");
    assert!(matches!(error, Error::HttpStatus { status: 503 }));
    assert!(stream.has_pending_operation());

    block_on(stream.retry_pending()).expect("replayed rollover");
    let calls = transport.calls();
    assert_eq!(calls[3].body["sequence"], calls[4].body["sequence"]);
    assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
    assert_eq!(calls[3].body["content"], calls[4].body["content"]);
    assert_eq!(stream.pages().len(), 2);
}

#[test]
fn retry_replays_current_page_finish_before_creating_follow_up() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157541")),
        Ok(message_response("om_page_1")),
        Ok(ok_response()),
        Err(Error::Transport("page finish response lost".to_owned())),
        Ok(ok_response()),
        Ok(card_response("7355372766134157542")),
        Ok(message_response("om_page_2")),
        Ok(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");

    let error =
        block_on(stream.append("word ".repeat(12))).expect_err("ambiguous current-page finish");
    assert!(matches!(error, Error::Transport(_)));
    block_on(stream.retry_pending()).expect("replayed rollover");

    let calls = transport.calls();
    assert_eq!(calls[4].method, HttpMethod::Patch);
    assert_eq!(calls[5].method, HttpMethod::Patch);
    assert_eq!(calls[4].body["sequence"], 2);
    assert_eq!(calls[5].body["sequence"], 2);
    assert_eq!(calls[4].body["uuid"], calls[5].body["uuid"]);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.path().ends_with("/cardkit/v1/cards"))
            .count(),
        2
    );
}

#[test]
fn retry_replays_first_update_on_delivered_follow_up_page() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157551")),
        Ok(message_response("om_page_1")),
        Ok(ok_response()),
        Ok(ok_response()),
        Ok(card_response("7355372766134157552")),
        Ok(message_response("om_page_2")),
        Err(Error::Transport(
            "follow-up update response lost".to_owned(),
        )),
        Ok(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");

    let error =
        block_on(stream.append("word ".repeat(12))).expect_err("ambiguous follow-up update");
    assert!(matches!(error, Error::Transport(_)));
    assert_eq!(stream.pages().len(), 2);
    block_on(stream.retry_pending()).expect("replayed follow-up update");

    let calls = transport.calls();
    assert_eq!(calls[7].body["sequence"], 1);
    assert_eq!(calls[8].body["sequence"], 1);
    assert_eq!(calls[7].body["uuid"], calls[8].body["uuid"]);
}

#[test]
fn cancellation_during_rollover_update_keeps_the_stage_recoverable() {
    let transport = FakeTransport::scripted(vec![
        FakeResponse::ready(token_response()),
        FakeResponse::ready(card_response("7355372766134157581")),
        FakeResponse::ready(message_response("om_page_1")),
        FakeResponse::pending_once(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(card_response("7355372766134157582")),
        FakeResponse::ready(message_response("om_page_2")),
        FakeResponse::ready(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = started_small_stream(&sender);

    assert_pending_once(stream.append("word ".repeat(12)));
    assert!(stream.has_pending_operation());
    assert!(!stream.is_recovery_blocked());
    assert_eq!(stream.pages().len(), 1);
    block_on(stream.retry_pending()).expect("resume cancelled update stage");

    let calls = transport.calls();
    assert_eq!(calls[3].body["sequence"], 1);
    assert_eq!(calls[4].body["sequence"], 1);
    assert_eq!(calls[3].body["uuid"], calls[4].body["uuid"]);
    assert_eq!(stream.pages().len(), 2);
}

#[test]
fn cancellation_during_rollover_finish_keeps_the_stage_recoverable() {
    let transport = FakeTransport::scripted(vec![
        FakeResponse::ready(token_response()),
        FakeResponse::ready(card_response("7355372766134157591")),
        FakeResponse::ready(message_response("om_page_1")),
        FakeResponse::ready(ok_response()),
        FakeResponse::pending_once(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(card_response("7355372766134157592")),
        FakeResponse::ready(message_response("om_page_2")),
        FakeResponse::ready(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = started_small_stream(&sender);

    assert_pending_once(stream.append("word ".repeat(12)));
    assert!(stream.has_pending_operation());
    assert!(!stream.is_finished());
    block_on(stream.retry_pending()).expect("resume cancelled finish stage");

    let calls = transport.calls();
    assert_eq!(calls[4].body["sequence"], 2);
    assert_eq!(calls[5].body["sequence"], 2);
    assert_eq!(calls[4].body["uuid"], calls[5].body["uuid"]);
    assert_eq!(stream.pages().len(), 2);
}

#[test]
fn cancellation_during_follow_up_delivery_keeps_the_prepared_card() {
    let transport = FakeTransport::scripted(vec![
        FakeResponse::ready(token_response()),
        FakeResponse::ready(card_response("7355372766134157601")),
        FakeResponse::ready(message_response("om_page_1")),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(card_response("7355372766134157602")),
        FakeResponse::pending_once(message_response("om_page_2")),
        FakeResponse::ready(message_response("om_page_2")),
        FakeResponse::ready(ok_response()),
    ]);
    let sender = sender(&transport);
    let mut stream = started_small_stream(&sender);

    assert_pending_once(stream.append("word ".repeat(12)));
    assert!(stream.has_pending_operation());
    assert_eq!(stream.pages().len(), 1);
    block_on(stream.retry_pending()).expect("resume cancelled follow-up delivery");

    let calls = transport.calls();
    assert_eq!(calls[6].body["uuid"], calls[7].body["uuid"]);
    assert_eq!(calls[6].body["content"], calls[7].body["content"]);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.path().ends_with("/cardkit/v1/cards"))
            .count(),
        2
    );
    assert_eq!(stream.pages().len(), 2);
}

#[test]
fn cancellation_during_follow_up_card_creation_blocks_replay() {
    let transport = FakeTransport::scripted(vec![
        FakeResponse::ready(token_response()),
        FakeResponse::ready(card_response("7355372766134157603")),
        FakeResponse::ready(message_response("om_page_1")),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::pending_once(card_response("7355372766134157604")),
    ]);
    let sender = sender(&transport);
    let mut stream = started_small_stream(&sender);

    assert_pending_once(stream.append("word ".repeat(12)));
    assert!(!stream.has_pending_operation());
    assert!(stream.is_recovery_blocked());
    assert_eq!(stream.next_flush_in(), None);
    assert_eq!(stream.pages().len(), 1);

    let error = block_on(stream.retry_pending()).expect_err("cancelled create is ambiguous");
    assert!(matches!(error, Error::Validation(message) if message.contains("unknown outcome")));
    assert_eq!(transport.calls().len(), 6);
    assert_eq!(stream.pages().len(), 1);
}

#[test]
fn rejects_content_clear_after_delivered_or_buffered_output() {
    let delivered_transport = FakeTransport::http(vec![
        token_response(),
        card_response("7355372766134157561"),
        message_response("om_delivered"),
        ok_response(),
    ]);
    let delivered_sender = sender(&delivered_transport);
    let mut delivered = block_on(
        delivered_sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .start_continuing(),
    )
    .expect("delivered stream");
    block_on(delivered.append("A")).expect("delivered content");
    let error =
        block_on(delivered.set_content("")).expect_err("delivered content cannot be cleared");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(delivered.content(), Some("A"));
    assert_eq!(delivered_transport.calls().len(), 4);

    let buffered_transport = FakeTransport::http(vec![
        token_response(),
        card_response("7355372766134157562"),
        message_response("om_buffered"),
        ok_response(),
    ]);
    let buffered_sender = sender(&buffered_transport);
    let mut buffered = block_on(
        buffered_sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .start_continuing(),
    )
    .expect("buffered stream")
    .throttle(Duration::from_secs(60));
    block_on(buffered.append("A")).expect("first content");
    block_on(buffered.append("B")).expect("buffered content");
    let error = block_on(buffered.set_content("")).expect_err("buffered content cannot be cleared");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(buffered.content(), Some("AB"));
    assert_eq!(buffered_transport.calls().len(), 4);
}

#[test]
fn incremental_rollover_never_moves_previously_accepted_source() {
    let transport = FakeTransport::http(vec![
        token_response(),
        card_response("7355372766134157611"),
        message_response("om_page_1"),
        ok_response(),
        ok_response(),
        ok_response(),
        card_response("7355372766134157612"),
        message_response("om_page_2"),
        ok_response(),
    ]);
    let sender = sender(&transport);
    let initial = "abcdefghijklmnopqrstuvwx\n\n1234567";
    assert_eq!(initial.len(), 33);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(40)
            .start_continuing(),
    )
    .expect("continuing stream")
    .throttle(Duration::from_secs(60));

    block_on(stream.append(initial)).expect("initial page content");
    let error = block_on(stream.set_content(&initial[..20]))
        .expect_err("accepted nonempty source cannot be truncated");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(transport.calls().len(), 4);

    block_on(stream.append("abcdefghij")).expect("incremental rollover");
    let calls = transport.calls();
    let finalized = calls[4].body["content"]
        .as_str()
        .expect("finalized first-page content");
    assert!(finalized.starts_with(initial));
    assert!(finalized.len() >= initial.len());
    assert_eq!(calls[5].method, HttpMethod::Patch);
    assert_eq!(stream.pages().len(), 2);
}

#[test]
fn invalid_snapshot_does_not_replay_a_pending_rollover() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157675")),
        Ok(message_response("om_pending_validation")),
        Err(Error::Transport("page update response lost".to_owned())),
    ]);
    let sender = sender(&transport);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");
    let source = "word ".repeat(12);

    let error = block_on(stream.append(&source)).expect_err("pending rollover");
    assert!(matches!(error, Error::Transport(_)));
    let calls_before_invalid_input = transport.calls().len();

    let error = block_on(stream.set_content("replacement"))
        .expect_err("non-monotonic snapshot is rejected locally");
    assert!(matches!(error, Error::Validation(_)));
    assert_eq!(transport.calls().len(), calls_before_invalid_input);

    assert!(stream.has_pending_operation());
    assert_eq!(stream.content(), Some(source.as_str()));
}

#[test]
fn consumes_a_retained_plan_across_multiple_follow_up_pages() {
    let transport = FakeTransport::http(vec![
        token_response(),
        card_response("7355372766134157631"),
        message_response("om_page_1"),
        ok_response(),
        ok_response(),
        card_response("7355372766134157632"),
        message_response("om_page_2"),
        ok_response(),
        ok_response(),
        card_response("7355372766134157633"),
        message_response("om_page_3"),
        ok_response(),
    ]);
    let sender = sender(&transport);
    let source = "x".repeat(40);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(16)
            .start_continuing(),
    )
    .expect("continuing stream");

    block_on(stream.append(&source)).expect("three-page continuation");

    let calls = transport.calls();
    let reconstructed = [3, 7, 11]
        .into_iter()
        .map(|index| calls[index].body["content"].as_str().expect("page content"))
        .collect::<String>();
    assert_eq!(reconstructed, source);
    assert_eq!(stream.pages().len(), 3);
    assert_eq!(calls[3].body["sequence"], 1);
    assert_eq!(calls[4].body["sequence"], 2);
    assert_eq!(calls[7].body["sequence"], 1);
    assert_eq!(calls[8].body["sequence"], 2);
    assert_eq!(calls[11].body["sequence"], 1);
}

#[test]
fn retains_later_pages_when_the_second_page_finish_needs_retry() {
    let transport = FakeTransport::new(vec![
        Ok(token_response()),
        Ok(card_response("7355372766134157641")),
        Ok(message_response("om_page_1")),
        Ok(ok_response()),
        Ok(ok_response()),
        Ok(card_response("7355372766134157642")),
        Ok(message_response("om_page_2")),
        Ok(ok_response()),
        Err(Error::Transport(
            "second-page finish response lost".to_owned(),
        )),
        Ok(ok_response()),
        Ok(card_response("7355372766134157643")),
        Ok(message_response("om_page_3")),
        Ok(ok_response()),
    ]);
    let sender = sender(&transport);
    let source = "x".repeat(40);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(16)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream");

    let error = block_on(stream.append(&source)).expect_err("ambiguous second-page finish");
    assert!(matches!(error, Error::Transport(_)));
    assert!(stream.has_pending_operation());
    assert_eq!(stream.pages().len(), 2);

    block_on(stream.retry_pending()).expect("resume retained final page");

    let calls = transport.calls();
    assert_eq!(calls[8].body["sequence"], 2);
    assert_eq!(calls[9].body["sequence"], 2);
    assert_eq!(calls[8].body["uuid"], calls[9].body["uuid"]);
    assert_eq!(calls[12].body["sequence"], 1);
    assert_eq!(stream.pages().len(), 3);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.path().ends_with("/cardkit/v1/cards"))
            .count(),
        3
    );
    let reconstructed = [3, 7, 12]
        .into_iter()
        .map(|index| calls[index].body["content"].as_str().expect("page content"))
        .collect::<String>();
    assert_eq!(reconstructed, source);
}

#[test]
fn cancellation_during_third_page_delivery_keeps_the_retained_plan() {
    let transport = FakeTransport::scripted(vec![
        FakeResponse::ready(token_response()),
        FakeResponse::ready(card_response("7355372766134157651")),
        FakeResponse::ready(message_response("om_page_1")),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(card_response("7355372766134157652")),
        FakeResponse::ready(message_response("om_page_2")),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(ok_response()),
        FakeResponse::ready(card_response("7355372766134157653")),
        FakeResponse::pending_once(message_response("om_page_3")),
        FakeResponse::ready(message_response("om_page_3")),
        FakeResponse::ready(ok_response()),
    ]);
    let sender = sender(&transport);
    let source = "x".repeat(40);
    let mut stream = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(16)
            .start_continuing(),
    )
    .expect("continuing stream");

    assert_pending_once(stream.append(&source));
    assert!(stream.has_pending_operation());
    assert_eq!(stream.pages().len(), 2);

    block_on(stream.retry_pending()).expect("resume third-page delivery");

    let calls = transport.calls();
    assert_eq!(calls[10].body["uuid"], calls[11].body["uuid"]);
    assert_eq!(calls[10].body["content"], calls[11].body["content"]);
    assert_eq!(stream.pages().len(), 3);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.url.path().ends_with("/cardkit/v1/cards"))
            .count(),
        3
    );
    let reconstructed = [3, 7, 12]
        .into_iter()
        .map(|index| calls[index].body["content"].as_str().expect("page content"))
        .collect::<String>();
    assert_eq!(reconstructed, source);
}

#[test]
fn rejects_zero_continuation_limit_without_remote_calls() {
    let transport = FakeTransport::http(vec![]);
    let sender = sender(&transport);

    let error = block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(0)
            .start_continuing(),
    )
    .expect_err("zero limit");

    assert!(matches!(error, Error::Validation(_)));
    assert!(transport.calls().is_empty());
}

fn sender(transport: &FakeTransport) -> MessageSender<FakeTransport> {
    let client = crate::lark_openapi::OpenApiClient::new(
        ChannelConfig::new("cli_a", "secret"),
        transport.clone(),
    );
    MessageSender::new(client)
}

fn started_small_stream<'a>(
    sender: &'a MessageSender<FakeTransport>,
) -> ContinuingMarkdownStream<'a, FakeTransport> {
    block_on(
        sender
            .markdown_stream_message(Recipient::Chat("oc_123".to_owned()))
            .continuation_max_page_chars(32)
            .max_attempts(1)
            .start_continuing(),
    )
    .expect("continuing stream")
}
