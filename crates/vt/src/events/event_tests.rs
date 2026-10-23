//! Tests of the event batch and its spans.

use std::time::Instant;

use super::*;
use crate::grid::{RowId, RowsScrolled, ScrollReport, Size};
use crate::terminal::{Config, OscRoute, Terminal};

fn terminal() -> Terminal {
    Terminal::new(Size { rows: 4, cols: 20 }, Config::default())
}

fn feed(term: &mut Terminal, batch: &mut EventBatch, bytes: &[u8]) {
    term.feed(bytes, batch, Instant::now());
}

#[test]
fn events_in_byte_order_repaint_last() {
    let mut term = terminal();
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, b"\x1b]2;a\x07\x07\x1b[c");
    let events = batch.events();
    assert_eq!(events.len(), 4);
    assert!(matches!(&events[0], VtEvent::Title(span) if batch.str(*span) == "a"));
    assert_eq!(events[1], VtEvent::Bell);
    assert!(matches!(&events[2], VtEvent::Reply(span) if batch.bytes(*span) == b"\x1b[?62;4;22c"));
    assert_eq!(events[3], VtEvent::Repaint);
}

#[test]
fn repaint_once_and_only_when_dispatched() {
    let mut term = terminal();
    let mut batch = EventBatch::new();
    feed(&mut term, &mut batch, b"a");
    feed(&mut term, &mut batch, b"b");
    assert_eq!(batch.events(), [VtEvent::Repaint]);
    feed(&mut term, &mut batch, b"\x1b");
    assert!(batch.is_empty());
    feed(&mut term, &mut batch, b"");
    assert!(batch.is_empty());
}

#[test]
fn spans_read_back_and_never_panic() {
    let mut batch = EventBatch::new();
    assert!(batch.push_title(b"hello"));
    batch.push_reply(b"\x00\xff");
    batch.push_osc(7, &[b"7", b"a", b""], StringTerm::St, false);
    let events = batch.events();
    let VtEvent::Title(title) = events[0] else {
        panic!("title first");
    };
    let VtEvent::Reply(reply) = events[1] else {
        panic!("reply second");
    };
    let VtEvent::Osc { params, .. } = events[2] else {
        panic!("osc third");
    };
    assert_eq!(batch.str(title), "hello");
    assert_eq!(batch.bytes(reply), b"\x00\xff");
    let read: Vec<&[u8]> = batch.params(params).collect();
    assert_eq!(read, [&b"7"[..], b"a", b""]);

    let other = EventBatch::new();
    assert_eq!(other.str(title), "");
    assert!(other.bytes(reply).is_empty());
    assert_eq!(other.params(params).count(), 0);
    // A span that ends past the arena or splits a character reads as nothing.
    let mut small = EventBatch::new();
    small.push_title("é".as_bytes());
    let VtEvent::Title(accent) = small.events()[0] else {
        panic!("title");
    };
    assert_eq!(small.str(accent), "é");
    small.clear();
    small.push_reply(b"\xff\xff");
    assert_eq!(small.str(accent), "");
}

#[test]
fn non_utf8_text_is_dropped_at_insert() {
    let mut batch = EventBatch::new();
    batch.push_reply(b"xyz");
    let before = (batch.len(), batch.mark());
    assert!(!batch.push_text(b"\xff", VtEvent::Title));
    assert!(!batch.push_title(b"\xc3"));
    assert_eq!((batch.len(), batch.mark()), before);
}

#[test]
fn finish_trimmed_rewinds_on_invalid_utf8() {
    let mut batch = EventBatch::new();
    batch.extend(b"keep");
    let mark = batch.mark();
    batch.extend(b" \xffbad ");
    assert_eq!(batch.finish_trimmed(mark), None);
    assert_eq!(batch.mark(), mark);
    batch.extend(b"  text \n");
    let span = batch.finish_trimmed(mark).expect("valid text");
    assert_eq!(batch.str(span), "text");
}

#[test]
fn finish_lossy_repairs_invalid_utf8() {
    let mut batch = EventBatch::new();
    let mark = batch.mark();
    batch.extend(b"a\xffb");
    let span = batch.finish_lossy(mark);
    assert_eq!(batch.str(span), "a\u{FFFD}b");
}

#[test]
fn clear_shrinks_past_the_soft_caps() {
    let mut batch = EventBatch::new();
    batch.push_reply(&vec![b'x'; 2 << 20]);
    for _ in 0..10_000 {
        batch.push(VtEvent::Bell);
    }
    let parts: Vec<&[u8]> = vec![b"x"; 10_000];
    batch.push_osc(1, &parts, StringTerm::Bel, false);
    assert!(batch.arena_capacity() > EVENT_ARENA_SOFT);
    assert!(batch.event_capacity() > EVENT_LIST_SOFT);
    assert!(batch.param_capacity() > EVENT_LIST_SOFT);
    batch.clear();
    assert!(batch.is_empty());
    assert!(batch.arena_capacity() <= EVENT_ARENA_SOFT);
    assert!(batch.event_capacity() <= EVENT_LIST_SOFT);
    assert!(batch.param_capacity() <= EVENT_LIST_SOFT);
}

#[test]
fn steady_feed_keeps_capacities() {
    let mut term = terminal();
    let mut batch = EventBatch::new();
    let mut capacities = None;
    for round in 0..1000 {
        feed(&mut term, &mut batch, b"line\r\n\x1b]0;t\x07");
        let now = (
            batch.arena_capacity(),
            batch.event_capacity(),
            batch.param_capacity(),
        );
        if round == 10 {
            capacities = Some(now);
        } else if round > 10 {
            assert_eq!(Some(now), capacities, "round {round}");
        }
    }
}

#[test]
fn scroll_report_maps_to_events() {
    let mut batch = EventBatch::new();
    let moved = RowsScrolled {
        top: RowId(2),
        bottom: RowId(5),
        delta: -1,
    };
    batch.push_scroll_report(&ScrollReport {
        scrolled: Some(moved),
        trimmed: Some(RowId(9)),
        history_rows: 1,
    });
    assert_eq!(
        batch.events(),
        [
            VtEvent::RowsScrolled(moved),
            VtEvent::RowsTrimmed { oldest: RowId(9) }
        ]
    );
    batch.clear();
    batch.push_scroll_report(&ScrollReport::NONE);
    assert!(batch.is_empty());
}

#[test]
fn public_push_methods_build_a_batch() {
    let mut batch = EventBatch::new();
    assert!(batch.push_title(b"t"));
    assert!(batch.push_clipboard_store(ClipboardKind::Primary, "é".as_bytes()));
    assert!(!batch.push_clipboard_store(ClipboardKind::Primary, b"\xff"));
    batch.push_reply(b"r");
    batch.push_osc(9, &[b"9", b"x"], StringTerm::Bel, true);
    batch.push(VtEvent::Bell);
    batch.push_repaint();
    batch.push_repaint();
    batch.push(VtEvent::Repaint);
    assert_eq!(batch.len(), 6);
    assert_eq!(batch.iter().count(), 6);
    let kinds: Vec<&str> = batch
        .iter()
        .map(|event| match event {
            VtEvent::Title(_) => "title",
            VtEvent::ClipboardStore { .. } => "clipboard",
            VtEvent::Reply(_) => "reply",
            VtEvent::Osc { .. } => "osc",
            VtEvent::Bell => "bell",
            VtEvent::Repaint => "repaint",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        ["title", "clipboard", "reply", "osc", "bell", "repaint"]
    );
    let VtEvent::Osc {
        code,
        params,
        terminator,
        truncated,
    } = &batch.events()[3]
    else {
        panic!("osc");
    };
    assert_eq!((*code, *terminator, *truncated), (9, StringTerm::Bel, true));
    assert_eq!(batch.params(*params).count(), 2);
}

#[test]
fn forwarded_osc_resplits_the_16th_param() {
    let mut config = Config::default();
    config.osc_routes.route(7501, OscRoute::Forward);
    let mut term = Terminal::new(Size { rows: 4, cols: 20 }, config);
    let mut batch = EventBatch::new();
    let params: Vec<String> = (1..=20).map(|n| n.to_string()).collect();
    let stream = format!("\x1b]7501;{}\x07", params.join(";"));
    feed(&mut term, &mut batch, stream.as_bytes());
    let VtEvent::Osc {
        code,
        params: spans,
        ..
    } = &batch.events()[0]
    else {
        panic!("osc first");
    };
    assert_eq!(*code, 7501);
    let read: Vec<String> = batch
        .params(*spans)
        .map(|param| String::from_utf8_lossy(param).into_owned())
        .collect();
    let mut expected = vec!["7501".to_owned()];
    expected.extend(params);
    assert_eq!(read, expected);
    assert_eq!(read.len(), 21);
}

#[test]
fn base64_decodes_canonical_input_only() {
    let mut batch = EventBatch::new();
    for (encoded, text) in [
        (&b"Zm9v"[..], "foo"),
        (b"Zm8=", "fo"),
        (b"Zg==", "f"),
        (b"", ""),
    ] {
        let span = batch.decode_base64(encoded).expect("canonical");
        assert_eq!(batch.str(span), text);
    }
    let kept = batch.mark();
    for bad in [
        &b"Zm9"[..],
        b"Zm=v",
        b"Z===",
        b"Zg==Zg==",
        b"Zm9v!",
        b"/w==",
        b"Zm9v\n",
    ] {
        assert_eq!(batch.decode_base64(bad), None, "{bad:?}");
        assert_eq!(batch.mark(), kept, "{bad:?} rewinds the arena");
    }
}
