//! Tests of OSC 22: setting a pointer shape, the stack of each screen, and the queries.

use std::borrow::Cow;

use super::pointer::{POINTER_NAME_MAX, POINTER_QUERY_MAX, POINTER_STACK_MAX};
use super::terminal_tests::Session;
use super::*;
use crate::events::VtEvent;

/// Feeds `OSC 22 ; body ST`.
fn send(session: &mut Session, body: &str) -> FeedStats {
    session.feed(format!("\x1b]22;{body}\x1b\\").as_bytes())
}

/// The pointer events of the last feed: a name, or `None` for a reset.
fn pointers(session: &Session) -> Vec<Option<String>> {
    let batch = &session.batch;
    batch
        .iter()
        .filter_map(|event| match event {
            VtEvent::Pointer(span) => Some(Some(batch.str(*span).to_owned())),
            VtEvent::PointerReset => Some(None),
            _ => None,
        })
        .collect()
}

fn shape(name: &str) -> Option<String> {
    Some(name.to_owned())
}

// ---- Setting ----

#[test]
fn a_name_sets_the_shape_with_or_without_the_equals_sign() {
    let mut session = Session::new(10, 4);
    assert_eq!(session.term.pointer_shape(), None);
    for body in ["pointer", "=wait"] {
        let stats = send(&mut session, body);
        let name = body.trim_start_matches('=');
        assert_eq!(pointers(&session), [shape(name)]);
        assert_eq!(session.term.pointer_shape(), Some(name));
        assert_eq!(stats.unhandled_sequences, 0);
    }
}

#[test]
fn an_empty_name_gives_the_pointer_back() {
    for body in ["", "="] {
        let mut session = Session::new(10, 4);
        send(&mut session, "wait");
        let stats = send(&mut session, body);
        assert_eq!(pointers(&session), [None]);
        assert_eq!(session.term.pointer_shape(), None);
        assert_eq!(stats.unhandled_sequences, 0);
    }
}

#[test]
fn of_a_list_the_last_name_is_set() {
    let mut session = Session::new(10, 4);
    send(&mut session, "wait,help");
    assert_eq!(pointers(&session), [shape("help")]);
}

#[test]
fn a_name_the_embedder_did_not_list_is_still_reported() {
    let mut session = Session::new(10, 4);
    let stats = send(&mut session, "left_ptr");
    assert_eq!(pointers(&session), [shape("left_ptr")]);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn text_that_is_no_shape_name_changes_nothing() {
    let mut session = Session::new(10, 4);
    send(&mut session, "wait");
    let too_long = "x".repeat(POINTER_NAME_MAX + 1);
    for body in [
        "Wait",
        "a b",
        "a/b",
        "wait,",
        ">help,,grab",
        ">",
        too_long.as_str(),
    ] {
        let stats = send(&mut session, body);
        assert!(pointers(&session).is_empty(), "{body:?}");
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
        assert_eq!(session.term.pointer_shape(), Some("wait"), "{body:?}");
    }
    let longest = "x".repeat(POINTER_NAME_MAX);
    send(&mut session, &longest);
    assert_eq!(session.term.pointer_shape(), Some(longest.as_str()));
    // No parameter at all, and bytes that are not UTF-8.
    assert_eq!(session.feed(b"\x1b]22\x07").unhandled_sequences, 1);
    assert_eq!(session.feed(b"\x1b]22;\xff\x07").unhandled_sequences, 1);
}

// ---- The stack ----

#[test]
fn pushed_shapes_come_back_in_reverse() {
    let mut session = Session::new(10, 4);
    send(&mut session, ">wait");
    assert_eq!(pointers(&session), [shape("wait")]);
    // Every name of a list is a level; the last one is shown.
    send(&mut session, ">help,grab");
    assert_eq!(pointers(&session), [shape("grab")]);
    for expected in [shape("help"), shape("wait"), None] {
        // The names after `<` are ignored.
        send(&mut session, "<ignored");
        assert_eq!(pointers(&session), [expected]);
    }
    // Nothing is left to pop.
    let stats = send(&mut session, "<");
    assert!(pointers(&session).is_empty());
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn setting_replaces_the_top_level() {
    let mut session = Session::new(10, 4);
    send(&mut session, ">wait");
    send(&mut session, "help");
    assert_eq!(pointers(&session), [shape("help")]);
    send(&mut session, "<");
    assert_eq!(pointers(&session), [None]);
}

#[test]
fn an_empty_name_keeps_its_level() {
    let mut session = Session::new(10, 4);
    send(&mut session, ">wait");
    send(&mut session, ">help");
    send(&mut session, "");
    assert_eq!(pointers(&session), [None]);
    // The level that asked for the terminal's own pointer is the one popped.
    send(&mut session, "<");
    assert_eq!(pointers(&session), [shape("wait")]);
}

#[test]
fn a_full_stack_drops_its_oldest_level() {
    let mut session = Session::new(10, 4);
    for index in 0..=POINTER_STACK_MAX {
        send(&mut session, &format!(">n{index}"));
    }
    for _ in 1..POINTER_STACK_MAX {
        send(&mut session, "<");
    }
    // `n0` went when the seventeenth name arrived: `n1` is the bottom.
    assert_eq!(session.term.pointer_shape(), Some("n1"));
    send(&mut session, "<");
    assert_eq!(pointers(&session), [None]);
}

// ---- The two screens ----

#[test]
fn each_screen_has_its_own_stack() {
    let mut session = Session::new(10, 4);
    send(&mut session, "wait");
    session.feed(b"\x1b[?1049h");
    assert_eq!(pointers(&session), [None]);
    send(&mut session, "help");
    session.feed(b"\x1b[?1049l");
    assert_eq!(pointers(&session), [shape("wait")]);
    assert_eq!(session.term.pointer_shape(), Some("wait"));
    // The alternate screen kept its shape.
    session.feed(b"\x1b[?47h");
    assert_eq!(pointers(&session), [shape("help")]);
}

#[test]
fn a_switch_between_equal_shapes_reports_nothing() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b[?1049h\x1b[?1049l");
    assert!(pointers(&session).is_empty());
    send(&mut session, "wait");
    session.feed(b"\x1b[?1049h");
    send(&mut session, "wait");
    session.feed(b"\x1b[?1049l");
    assert!(pointers(&session).is_empty());
}

#[test]
fn a_program_that_leaves_the_alternate_screen_leaves_no_shape_behind() {
    // A full-screen program that never gives the pointer back.
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b[?1049h\x1b]22;pointer\x07");
    session.feed(b"\x1b[?1049l");
    assert_eq!(pointers(&session), [None]);
    assert_eq!(session.term.pointer_shape(), None);
}

#[test]
fn a_reset_empties_both_stacks() {
    let mut session = Session::new(10, 4);
    send(&mut session, ">wait");
    session.feed(b"\x1b[?1049h");
    send(&mut session, ">help");
    session.feed(b"\x1bc");
    assert_eq!(pointers(&session), [None]);
    assert_eq!(session.term.pointer_shape(), None);
    // Neither screen has a level left.
    send(&mut session, "<");
    session.feed(b"\x1b[?1049h");
    send(&mut session, "<");
    assert!(pointers(&session).is_empty());
}

// ---- Queries ----

#[test]
fn a_query_answers_one_value_per_name() {
    let mut session = Session::new(10, 4);
    let stats = send(&mut session, "?pointer,crosshair,no-such-name,wait");
    assert_eq!(session.replies(), "\x1b]22;1,1,0,1\x1b\\");
    assert_eq!(stats.unhandled_sequences, 0);
    assert!(pointers(&session).is_empty());
    // The answer ends the way the query did.
    session.feed(b"\x1b]22;?text\x07");
    assert_eq!(session.replies(), "\x1b]22;1\x07");
    // Text that is no shape name is not supported, whatever it is.
    send(&mut session, "?,Wait,a b");
    assert_eq!(session.replies(), "\x1b]22;0,0,0\x1b\\");
}

#[test]
fn every_listed_css_name_is_supported_by_default() {
    let mut session = Session::new(10, 4);
    send(&mut session, &format!("?{}", PointerShapes::CSS.join(",")));
    let ones = vec!["1"; PointerShapes::CSS.len()].join(",");
    assert_eq!(session.replies(), format!("\x1b]22;{ones}\x1b\\"));
}

#[test]
fn the_current_shape_is_a_name_or_zero() {
    let mut session = Session::new(10, 4);
    send(&mut session, "?__current__");
    assert_eq!(session.replies(), "\x1b]22;0\x1b\\");
    send(&mut session, ">wait");
    send(&mut session, "?__current__,__current__");
    assert_eq!(session.replies(), "\x1b]22;wait,wait\x1b\\");
    // A level that asks for the terminal's own pointer is no shape either.
    send(&mut session, "");
    send(&mut session, "?__current__");
    assert_eq!(session.replies(), "\x1b]22;0\x1b\\");
}

#[test]
fn the_default_and_grabbed_shapes_come_from_the_config() {
    let mut session = Session::new(10, 4);
    send(&mut session, "?__default__,__grabbed__");
    assert_eq!(session.replies(), "\x1b]22;text,default\x1b\\");

    let config = Config {
        pointer_shapes: PointerShapes {
            supported: vec![Cow::Borrowed("left_ptr"), Cow::Borrowed("wait")],
            default_shape: Cow::Borrowed("left_ptr"),
            grabbed_shape: Cow::Borrowed("wait"),
        },
        ..Config::default()
    };
    let mut session = Session::with(config);
    send(
        &mut session,
        "?__default__,__grabbed__,left_ptr,wait,pointer",
    );
    assert_eq!(session.replies(), "\x1b]22;left_ptr,wait,1,1,0\x1b\\");
}

#[test]
fn config_names_that_cannot_go_into_a_reply_are_cleaned() {
    let config = Config {
        pointer_shapes: PointerShapes {
            supported: vec![
                Cow::Borrowed("wait"),
                Cow::Borrowed("bad\x1bname"),
                Cow::Borrowed(""),
            ],
            default_shape: Cow::Borrowed("a\x07b"),
            grabbed_shape: Cow::Owned("x".repeat(POINTER_NAME_MAX + 1)),
        },
        ..Config::default()
    };
    let session = Session::with(config);
    let shapes = &session.term.config().pointer_shapes;
    assert_eq!(shapes.supported, [Cow::Borrowed("wait")]);
    assert_eq!(shapes.default_shape, "text");
    assert_eq!(shapes.grabbed_shape, "default");
}

#[test]
fn a_query_without_names_or_with_too_many_gets_no_answer() {
    let mut session = Session::new(10, 4);
    let most = vec!["wait"; POINTER_QUERY_MAX].join(",");
    send(&mut session, &format!("?{most}"));
    assert_eq!(session.replies().matches('1').count(), POINTER_QUERY_MAX);
    for body in ["?".to_owned(), format!("?{most},wait")] {
        let stats = send(&mut session, &body);
        assert_eq!(session.replies(), "");
        assert_eq!(stats.unhandled_sequences, 1);
    }
}
