//! Tests of the query buffers, the DCS ceilings and the SGR serialisation DECRQSS uses.

use std::borrow::Cow;

use super::query::{DcsQuery, QUERY_MAX_BYTES, sanitize_product_name};
use super::terminal_tests::Session;
use super::*;

#[test]
fn product_names_are_cleaned_once() {
    let clean = |name: &str| sanitize_product_name(Some(Cow::Owned(name.to_owned())));
    assert_eq!(clean("Acme").as_deref(), Some("Acme"));
    assert_eq!(clean("A\x1bB\u{9b}C").as_deref(), Some("ABC"));
    assert_eq!(clean("\x01\x7f").as_deref(), None);
    assert_eq!(clean(&"é".repeat(40)).map(|name| name.len()), Some(64));
    assert_eq!(clean(&"é".repeat(33)).map(|name| name.len()), Some(64));
    assert_eq!(sanitize_product_name(None), None);
    assert!(matches!(clean("x"), Some(Cow::Owned(_))));
}

#[test]
fn the_terminal_stores_the_cleaned_name() {
    let config = Config {
        product_name: Some(Cow::Borrowed("A\x1bB")),
        ..Config::default()
    };
    let session = Session::with(config);
    assert_eq!(session.term.config().product_name.as_deref(), Some("AB"));
}

#[test]
fn query_buffer_is_reused_across_requests() {
    let mut session = Session::new(10, 3);
    session.feed(b"\x1bP+q544e;436f\x1b\\");
    let capacity = session.term.state.dcs_payload.capacity();
    assert!(capacity > 0);
    for _ in 0..10_000 {
        session.feed(b"\x1bP+q544e;436f\x1b\\");
    }
    assert_eq!(session.term.state.dcs_payload.capacity(), capacity);
    assert!(session.term.state.dcs_payload.is_empty());
    assert!(session.term.state.dcs_query.is_none());
}

#[test]
fn an_oversized_request_is_dropped_and_the_buffer_stays_bounded() {
    let mut session = Session::new(10, 3);
    for kind in [&b"$q"[..], b"+q"] {
        let mut stream = b"\x1bP".to_vec();
        stream.extend_from_slice(kind);
        stream.extend(std::iter::repeat_n(b'4', 9 * 1024));
        stream.extend_from_slice(b"\x1b\\");
        let stats = session.feed(&stream);
        assert_eq!(session.replies(), "");
        assert_eq!(stats.unhandled_sequences, 1);
        assert_eq!(stats.aborted_dcs, 0);
        assert!(session.term.state.dcs_payload.capacity() <= QUERY_MAX_BYTES);
        assert!(session.term.state.dcs_payload.is_empty());
    }
}

#[test]
fn a_request_at_the_ceiling_is_dropped_and_one_below_is_answered() {
    let mut session = Session::new(10, 3);
    let request = |len: usize| {
        let mut stream = b"\x1bP+q".to_vec();
        stream.extend(std::iter::repeat_n(b'z', len));
        stream.extend_from_slice(b"\x1b\\");
        stream
    };
    let stats = session.feed(&request(QUERY_MAX_BYTES));
    assert_eq!((stats.unhandled_sequences, session.replies().len()), (1, 0));
    let stats = session.feed(&request(QUERY_MAX_BYTES - 1));
    assert_eq!(
        stats.unhandled_sequences, 1,
        "the name is too long to answer"
    );
    assert!(session.replies().is_empty());
}

#[test]
fn an_aborted_query_answers_nothing() {
    let mut session = Session::new(10, 3);
    let stats = session.feed(b"\x1bP$qm\x18");
    assert_eq!(stats.aborted_dcs, 1);
    assert_eq!(session.replies(), "");
    assert!(session.term.state.dcs_payload.is_empty());
    assert!(session.term.state.dcs_query.is_none());
    let stats = session.feed(b"\x1bP+q544e\x1a");
    assert_eq!(stats.aborted_dcs, 1);
    assert_eq!(session.replies(), "");
}

#[test]
fn resets_clear_a_pending_query() {
    for reset in [&b"\x1bc"[..], b"\x1b[!p"] {
        let mut session = Session::new(10, 3);
        session.term.state.dcs_query = Some(DcsQuery::Decrqss);
        session.term.state.dcs_payload.extend_from_slice(b"mmm");
        session.feed(reset);
        assert!(session.term.state.dcs_query.is_none(), "{reset:?}");
        assert!(session.term.state.dcs_payload.is_empty(), "{reset:?}");
    }
}

#[test]
fn a_new_query_starts_with_an_empty_payload() {
    let mut session = Session::new(10, 3);
    session.feed(b"\x1bP$qr");
    session.feed(b"\x1b\\");
    assert_eq!(session.replies(), "\x1bP1$r1;3r\x1b\\");
    session.feed(b"\x1bP$qm\x1b\\");
    assert_eq!(session.replies(), "\x1bP1$r0m\x1b\\");
}

#[test]
fn query_payload_survives_chunking() {
    let mut session = Session::new(10, 3);
    for chunk in [&b"\x1bP$"[..], b"q", b"m", b"\x1b", b"\\"] {
        session.feed(chunk);
    }
    assert_eq!(session.term.stats().unhandled_sequences, 0);
    let mut all = Vec::new();
    session.feed(b"\x1bP$qm\x1b\\");
    all.extend_from_slice(session.replies().as_bytes());
    assert_eq!(all, b"\x1bP1$r0m\x1b\\");
}

#[test]
fn decrqss_sgr_round_trips_through_a_second_terminal() {
    let states = [
        "\x1b[1m",
        "\x1b[2;3m",
        "\x1b[4m",
        "\x1b[4:2m",
        "\x1b[4:3m",
        "\x1b[4:4m",
        "\x1b[4:5m",
        "\x1b[5;7;8;9;53m",
        "\x1b[6m",
        "\x1b[31;42m",
        "\x1b[91;102m",
        "\x1b[38;5;200;48;5;7m",
        "\x1b[38;2;1;2;3;48;2;4;5;6m",
        "\x1b[58;5;9m",
        "\x1b[58;2;7;8;9;1;4m",
        "\x1b[38;5;3;48;2;0;0;0;58;5;255;2;3;4:3m",
    ];
    assert!(states.len() >= 15);
    for state in states {
        let mut first = Session::new(10, 3);
        first.feed(state.as_bytes());
        first.feed(b"\x1bP$qm\x1b\\");
        let reply = first.replies();
        let parameters = reply
            .strip_prefix("\x1bP1$r")
            .and_then(|rest| rest.strip_suffix("m\x1b\\"))
            .expect("a SGR status string");
        let mut second = Session::new(10, 3);
        second.feed(format!("\x1b[{parameters}m").as_bytes());
        assert_eq!(second.term.style(), first.term.style(), "{state:?}");
    }
}

#[test]
fn replies_are_ascii_and_pushed_as_one_event_each() {
    let mut session = Session::new(10, 3);
    session.feed(b"\x1b[c\x1b[5n\x1bP+q544e;436f\x1b\\");
    let replies: Vec<_> = session
        .batch
        .iter()
        .filter(|event| matches!(event, crate::events::VtEvent::Reply(_)))
        .collect();
    assert_eq!(replies.len(), 4);
}
