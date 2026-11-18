//! Tests of OSC 99 desktop notifications and of OSC 777 `notify`.

use super::notification::{NOTIFICATION_ID_MAX, NOTIFICATION_TEXT_MAX, PENDING_NOTIFICATIONS_MAX};
use super::terminal_tests::Session;
use super::*;
use crate::events::VtEvent;

/// One OSC with `body`, ended by `ESC \`.
fn osc(body: &[u8]) -> Vec<u8> {
    [b"\x1b]", body, b"\x1b\\"].concat()
}

/// Feeds every body as its own OSC, all in one call.
fn send(session: &mut Session, bodies: &[&str]) -> FeedStats {
    let bytes: Vec<u8> = bodies
        .iter()
        .flat_map(|body| osc(body.as_bytes()))
        .collect();
    session.feed(&bytes)
}

/// `(title, body)` of every notification of the last feed.
fn shown(session: &Session) -> Vec<(String, String)> {
    let batch = &session.batch;
    batch
        .iter()
        .filter_map(|event| match event {
            VtEvent::Notification { title, body } => {
                Some((batch.str(*title).to_owned(), batch.str(*body).to_owned()))
            }
            _ => None,
        })
        .collect()
}

fn pair(title: &str, body: &str) -> (String, String) {
    (title.to_owned(), body.to_owned())
}

/// RFC 4648 base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in bytes.chunks(3) {
        let mut bits = 0u32;
        for (index, byte) in group.iter().enumerate() {
            bits |= u32::from(*byte) << (16 - 8 * index);
        }
        for index in 0..4 {
            if index <= group.len() {
                out.push(char::from(
                    ALPHABET[(bits >> (18 - 6 * index)) as usize & 63],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---- OSC 99: text ----

#[test]
fn one_sequence_is_a_notification_with_a_title() {
    let mut session = Session::new(10, 4);
    let stats = send(&mut session, &["99;;Hello world"]);
    assert_eq!(shown(&session), [pair("Hello world", "")]);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn a_title_and_a_body_arrive_in_two_sequences() {
    let mut session = Session::new(10, 4);
    let stats = send(&mut session, &["99;i=1:d=0;Hello world"]);
    assert!(shown(&session).is_empty());
    assert_eq!(stats.unhandled_sequences, 0);
    let stats = send(&mut session, &["99;i=1:p=body;This is cool"]);
    assert_eq!(shown(&session), [pair("Hello world", "This is cool")]);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn chunks_of_one_text_are_joined_and_keep_their_semicolons() {
    let mut session = Session::new(10, 4);
    send(
        &mut session,
        &[
            "99;i=1:d=0;Hel",
            "99;i=1:d=0:p=title;lo",
            "99;i=1:d=0:p=body;a;b;",
            "99;i=1:d=1:p=body;c",
        ],
    );
    assert_eq!(shown(&session), [pair("Hello", "a;b;c")]);
}

#[test]
fn a_body_without_a_title_is_the_title() {
    let mut session = Session::new(10, 4);
    send(&mut session, &["99;p=body;only this"]);
    assert_eq!(shown(&session), [pair("only this", "")]);
}

#[test]
fn a_notification_without_text_is_not_reported() {
    for body in ["99", "99;", "99;;", "99;i=1;", "99;i=1:p=body;"] {
        let mut session = Session::new(10, 4);
        let stats = send(&mut session, &[body]);
        assert!(shown(&session).is_empty(), "{body:?}");
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
    }
}

#[test]
fn chunks_without_an_id_are_one_notification() {
    let mut session = Session::new(10, 4);
    send(&mut session, &["99;d=0;Title", "99;p=body;Body"]);
    assert_eq!(shown(&session), [pair("Title", "Body")]);
}

#[test]
fn notifications_with_different_ids_do_not_mix() {
    let mut session = Session::new(10, 4);
    send(
        &mut session,
        &[
            "99;i=a:d=0;first",
            "99;i=b:d=0;second",
            "99;i=a:p=body;one",
            "99;i=b:p=body;two",
        ],
    );
    assert_eq!(
        shown(&session),
        [pair("first", "one"), pair("second", "two")]
    );
}

#[test]
fn a_chunk_may_end_inside_a_character() {
    let text = "h\u{e9}llo \u{2603}";
    for cut in 0..=text.len() {
        let (head, tail) = text.as_bytes().split_at(cut);
        let mut session = Session::new(10, 4);
        let mut bytes = osc(&[b"99;i=1:d=0;", head].concat());
        bytes.extend(osc(&[b"99;i=1;", tail].concat()));
        session.feed(&bytes);
        assert_eq!(shown(&session), [pair(text, "")], "cut at {cut}");
    }
}

// ---- OSC 99: base64 ----

#[test]
fn base64_text_is_decoded() {
    let mut session = Session::new(10, 4);
    let title = base64("h\u{e9}llo".as_bytes());
    let body = base64(b"line one\nline two");
    let stats = send(
        &mut session,
        &[
            &format!("99;i=1:d=0:e=1;{title}"),
            &format!("99;i=1:e=1:p=body;{body}"),
        ],
    );
    assert_eq!(shown(&session), [pair("h\u{e9}llo", "line one\nline two")]);
    assert_eq!(
        (stats.unhandled_sequences, stats.malformed_sequences),
        (0, 0)
    );
}

#[test]
fn text_encoded_chunk_by_chunk_is_joined() {
    // Each chunk carries its own padding, as the protocol asks of a sender that cuts first.
    let mut session = Session::new(10, 4);
    let (first, second) = (base64(b"Hell"), base64(b"o world"));
    assert!(first.ends_with("=="));
    send(
        &mut session,
        &[
            &format!("99;i=1:d=0:e=1;{first}"),
            &format!("99;i=1:e=1;{second}"),
        ],
    );
    assert_eq!(shown(&session), [pair("Hello world", "")]);
}

#[test]
fn one_encoding_cut_anywhere_is_joined_with_or_without_padding() {
    let text = "h\u{e9}llo \u{2603}!";
    let padded = base64(text.as_bytes());
    assert!(padded.ends_with('='));
    for encoded in [padded.as_str(), padded.trim_end_matches('=')] {
        for cut in 0..=encoded.len() {
            let (head, tail) = encoded.split_at(cut);
            let mut session = Session::new(10, 4);
            let stats = send(
                &mut session,
                &[
                    &format!("99;i=1:d=0:e=1;{head}"),
                    &format!("99;i=1:e=1;{tail}"),
                ],
            );
            assert_eq!(
                shown(&session),
                [pair(text, "")],
                "{encoded:?} cut at {cut}"
            );
            assert_eq!(stats.malformed_sequences, 0);
        }
    }
}

#[test]
fn text_that_is_not_base64_adds_nothing() {
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &["99;i=1:d=0;Title", "99;i=1:e=1:p=body;!!!!"],
    );
    assert_eq!(shown(&session), [pair("Title", "")]);
    assert_eq!(
        (stats.malformed_sequences, stats.unhandled_sequences),
        (1, 0)
    );
    // A `;` is not base64 either; with no text left there is nothing to report.
    let stats = send(&mut session, &["99;e=1;QUJD;REVG"]);
    assert!(shown(&session).is_empty());
    assert_eq!(
        (stats.malformed_sequences, stats.unhandled_sequences),
        (1, 1)
    );
}

#[test]
fn a_refused_chunk_leaves_the_group_before_it_open() {
    // "QUJD" is "ABC": the second chunk is refused whole, the third ends the group.
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &["99;i=1:d=0:e=1;QU", "99;i=1:d=0:e=1;J!", "99;i=1:e=1;JD"],
    );
    assert_eq!(shown(&session), [pair("ABC", "")]);
    assert_eq!(stats.malformed_sequences, 1);
}

// ---- OSC 99: the query ----

#[test]
fn the_query_is_answered_with_what_is_understood() {
    let mut session = Session::new(10, 4);
    let stats = session.feed(b"\x1b]99;i=abc-1:p=?;\x1b\\");
    assert_eq!(
        session.replies(),
        "\x1b]99;i=abc-1:p=?;o=always:p=title,body,?\x1b\\"
    );
    assert_eq!(stats.unhandled_sequences, 0);
    assert!(shown(&session).is_empty());
    // The answer ends the way the query did.
    session.feed(b"\x1b]99;i=abc-1:p=?;\x07");
    assert_eq!(
        session.replies(),
        "\x1b]99;i=abc-1:p=?;o=always:p=title,body,?\x07"
    );
}

#[test]
fn the_query_answer_names_id_0_when_none_was_sent() {
    let mut session = Session::new(10, 4);
    // With and without the second `;`.
    for query in [&b"\x1b]99;p=?;\x07"[..], b"\x1b]99;p=?\x07"] {
        session.feed(query);
        assert_eq!(
            session.replies(),
            "\x1b]99;i=0:p=?;o=always:p=title,body,?\x07"
        );
    }
}

#[test]
fn an_id_is_echoed_without_the_characters_an_id_may_not_hold() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b]99;i=a!b\x01~c/_+.9:p=?;\x07");
    assert_eq!(
        session.replies(),
        "\x1b]99;i=abc_+.9:p=?;o=always:p=title,body,?\x07"
    );
}

#[test]
fn an_id_past_its_cap_is_refused() {
    let mut session = Session::new(10, 4);
    let longest = "x".repeat(NOTIFICATION_ID_MAX);
    let stats = send(&mut session, &[&format!("99;i={longest};Hi")]);
    assert_eq!(shown(&session), [pair("Hi", "")]);
    assert_eq!(stats.unhandled_sequences, 0);
    for body in [
        format!("99;i={longest}x;Hi"),
        format!("99;i={longest}x:p=?;"),
    ] {
        let stats = send(&mut session, &[&body]);
        assert!(shown(&session).is_empty());
        assert_eq!(session.replies(), "");
        assert_eq!(stats.unhandled_sequences, 1);
    }
}

// ---- OSC 99: what has no event ----

#[test]
fn keys_without_an_event_are_skipped() {
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &["99;i=1:u=2:a=report,-focus:o=unfocused:c=1:w=500:f=Zm9v:s=c2lsZW50:zz=1:=:x;Hi"],
    );
    assert_eq!(shown(&session), [pair("Hi", "")]);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn payloads_without_an_event_are_counted_and_keep_the_notification() {
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &[
            "99;i=1:d=0;Title",
            "99;i=1:d=0:p=icon:e=1;AAAA",
            "99;i=1:p=buttons;Yes",
        ],
    );
    assert_eq!(shown(&session), [pair("Title", "")]);
    assert_eq!(stats.unhandled_sequences, 2);
    // Alone, such a payload is one unhandled sequence, not two.
    let stats = send(&mut session, &["99;p=icon:e=1;AAAA"]);
    assert!(shown(&session).is_empty());
    assert_eq!(stats.unhandled_sequences, 1);
}

#[test]
fn close_and_alive_are_counted_and_touch_nothing() {
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &[
            "99;i=1:d=0;Title",
            "99;i=1:p=close;",
            "99;i=1:p=alive;",
            "99;i=1:p=body;Body",
        ],
    );
    assert_eq!(shown(&session), [pair("Title", "Body")]);
    assert_eq!(session.replies(), "");
    assert_eq!(stats.unhandled_sequences, 2);
}

// ---- OSC 99: ceilings ----

#[test]
fn text_past_its_cap_is_cut_at_a_character() {
    // 1 + 2n bytes: the cap falls inside a two-byte character.
    let chunk = "\u{e9}".repeat(900);
    let mut session = Session::new(10, 4);
    let first = send(&mut session, &[&format!("99;i=1:d=0;a{chunk}")]);
    let second = send(&mut session, &[&format!("99;i=1:d=0;{chunk}")]);
    let third = send(&mut session, &[&format!("99;i=1:d=0;{chunk}")]);
    assert_eq!(
        [first, second, third].map(|stats| stats.unhandled_sequences),
        [0, 0, 1]
    );
    let stats = send(&mut session, &["99;i=1:p=body;kept"]);
    let notifications = shown(&session);
    let (title, body) = &notifications[0];
    assert_eq!(title.len(), NOTIFICATION_TEXT_MAX - 1);
    assert!(title.starts_with('a') && title.ends_with('\u{e9}'));
    assert_eq!(body, "kept");
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn base64_text_past_its_cap_is_cut() {
    let chunk = base64(&[b'x'; 1400]);
    let mut session = Session::new(10, 4);
    let mut cut = 0;
    for _ in 0..3 {
        cut += send(&mut session, &[&format!("99;i=1:d=0:e=1:p=body;{chunk}")]).unhandled_sequences;
    }
    assert_eq!(cut, 1);
    send(&mut session, &["99;i=1;Title"]);
    assert_eq!(
        shown(&session),
        [pair("Title", &"x".repeat(NOTIFICATION_TEXT_MAX))]
    );
}

#[test]
fn only_so_many_notifications_wait_for_their_end() {
    let mut session = Session::new(10, 4);
    for index in 0..=PENDING_NOTIFICATIONS_MAX {
        send(&mut session, &[&format!("99;i=n{index}:d=0;title {index}")]);
    }
    // The one that waited longest is gone: its body arrives alone.
    send(&mut session, &["99;i=n0:p=body;late"]);
    assert_eq!(shown(&session), [pair("late", "")]);
    send(&mut session, &["99;i=n1:p=body;on time"]);
    assert_eq!(shown(&session), [pair("title 1", "on time")]);
}

#[test]
fn a_reset_forgets_unfinished_notifications() {
    let mut session = Session::new(10, 4);
    send(&mut session, &["99;i=1:d=0;Title"]);
    session.feed(b"\x1bc");
    send(&mut session, &["99;i=1:p=body;Body"]);
    assert_eq!(shown(&session), [pair("Body", "")]);
}

// ---- OSC 99: routes ----

#[test]
fn a_dropped_route_answers_nothing_and_a_forwarded_one_is_the_embedders() {
    let mut config = Config::default();
    config.osc_routes.route(99, OscRoute::Drop);
    let mut session = Session::with(config);
    let stats = send(&mut session, &["99;i=1:p=?;", "99;;Hi"]);
    assert_eq!(session.replies(), "");
    assert!(shown(&session).is_empty());
    assert_eq!(stats.unhandled_sequences, 2);

    let mut config = Config::default();
    config.osc_routes.route(99, OscRoute::Forward);
    let mut session = Session::with(config);
    let stats = send(&mut session, &["99;i=1:p=?;", "99;;Hi"]);
    assert_eq!(session.replies(), "");
    assert!(shown(&session).is_empty());
    assert_eq!(stats.unhandled_sequences, 0);
    let forwarded = session
        .batch
        .iter()
        .filter(|event| matches!(event, VtEvent::Osc { code: 99, .. }))
        .count();
    assert_eq!(forwarded, 2);
}

// ---- OSC 777 ----

#[test]
fn osc_777_notify_is_a_notification() {
    let mut session = Session::new(10, 4);
    for (body, expected) in [
        ("777;notify;Title;Body", pair("Title", "Body")),
        ("777;notify;Title;a;b", pair("Title", "a;b")),
        ("777;notify;Title", pair("Title", "")),
        ("777;notify;Title;", pair("Title", "")),
        ("777;notify;;Body", pair("", "Body")),
    ] {
        let stats = send(&mut session, &[body]);
        assert_eq!(shown(&session), [expected], "{body:?}");
        assert_eq!(stats.unhandled_sequences, 0, "{body:?}");
    }
}

#[test]
fn osc_777_rejects_nothing_to_say_and_other_commands() {
    let mut session = Session::new(10, 4);
    for body in [
        "777",
        "777;notify",
        "777;notify;",
        "777;notify;;",
        "777;precmd",
        "777;Notify;a;b",
    ] {
        let stats = send(&mut session, &[body]);
        assert!(shown(&session).is_empty(), "{body:?}");
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
    }
}
