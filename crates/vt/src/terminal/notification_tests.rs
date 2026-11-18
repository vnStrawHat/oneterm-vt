//! Tests of OSC 99 desktop notifications and of OSC 777 `notify`.

use std::borrow::Cow;

use super::notification::{
    NOTIFICATION_ICON_MAX, NOTIFICATION_ID_MAX, NOTIFICATION_LIST_MAX, NOTIFICATION_NAME_MAX,
    NOTIFICATION_TEXT_MAX, PENDING_NOTIFICATIONS_MAX,
};
use super::terminal_tests::Session;
use super::*;
use crate::events::{
    Notification, NotificationExpiry, NotificationOccasion, NotificationUrgency, ParamSpans,
    VtEvent,
};

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
            VtEvent::Notification(at) => {
                let notification = batch.notification(*at);
                Some((
                    batch.str(notification.title).to_owned(),
                    batch.str(notification.body).to_owned(),
                ))
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

// ---- OSC 99: the keys ----

/// The one notification of the last feed.
fn only(session: &Session) -> &Notification {
    let mut found = session.batch.iter().filter_map(|event| match event {
        VtEvent::Notification(at) => Some(session.batch.notification(*at)),
        _ => None,
    });
    let notification = found.next().expect("a notification");
    assert!(found.next().is_none(), "one notification");
    notification
}

fn texts(session: &Session, spans: ParamSpans) -> Vec<String> {
    session.batch.strs(spans).map(str::to_owned).collect()
}

#[test]
fn a_notification_without_keys_has_the_protocol_defaults() {
    let mut session = Session::new(10, 4);
    send(&mut session, &["99;;Hi"]);
    let notification = only(&session);
    let batch = &session.batch;
    assert_eq!(batch.str(notification.id), "");
    assert_eq!(batch.str(notification.app_name), "");
    assert_eq!(batch.str(notification.icon_id), "");
    assert_eq!(batch.str(notification.sound), "");
    assert!(batch.bytes(notification.icon).is_empty());
    assert!(texts(&session, notification.kinds).is_empty());
    assert!(texts(&session, notification.icon_names).is_empty());
    assert!(texts(&session, notification.buttons).is_empty());
    assert_eq!(notification.urgency, None);
    assert_eq!(notification.occasion, NotificationOccasion::Always);
    assert_eq!(notification.expiry, NotificationExpiry::Platform);
    assert!(notification.focus_on_activation);
    assert!(!notification.report_activation);
    assert!(!notification.report_close);
}

#[test]
fn every_key_reaches_the_embedder() {
    let mut session = Session::new(10, 4);
    let body = format!(
        "99;i=n1:a=report,-focus:c=1:o=invisible:u=2:w=1500:f={}:t={}:t={}:n={}:n={}:g=icon-1:s={}:zz=1:=:x;Title",
        base64(b"foo-app"),
        base64(b"im.received"),
        base64(b"transfer"),
        base64(b"error"),
        base64(b"foo"),
        base64(b"silent"),
    );
    let stats = send(&mut session, &[&body]);
    assert_eq!(
        (stats.unhandled_sequences, stats.malformed_sequences),
        (0, 0)
    );
    let notification = only(&session);
    let batch = &session.batch;
    assert_eq!(batch.str(notification.id), "n1");
    assert_eq!(batch.str(notification.title), "Title");
    assert_eq!(batch.str(notification.app_name), "foo-app");
    assert_eq!(
        texts(&session, notification.kinds),
        ["im.received", "transfer"]
    );
    assert_eq!(texts(&session, notification.icon_names), ["error", "foo"]);
    assert_eq!(batch.str(notification.icon_id), "icon-1");
    assert_eq!(batch.str(notification.sound), "silent");
    assert_eq!(notification.urgency, Some(NotificationUrgency::Critical));
    assert_eq!(notification.occasion, NotificationOccasion::Invisible);
    assert_eq!(notification.expiry, NotificationExpiry::After(1500));
    assert!(!notification.focus_on_activation);
    assert!(notification.report_activation);
    assert!(notification.report_close);
}

#[test]
fn keys_come_with_any_chunk_and_a_later_one_wins() {
    let mut session = Session::new(10, 4);
    send(
        &mut session,
        &[
            "99;i=1:d=0:u=0:a=report:w=-1;Title",
            "99;i=1:d=0:p=body:u=2:c=1;Body",
            "99;i=1:a=-report:w=0:c=0;",
        ],
    );
    let notification = only(&session);
    assert_eq!(notification.urgency, Some(NotificationUrgency::Critical));
    assert_eq!(notification.expiry, NotificationExpiry::Never);
    assert!(notification.focus_on_activation);
    assert!(!notification.report_activation);
    assert!(!notification.report_close);
}

#[test]
fn a_value_the_protocol_does_not_define_is_counted_and_skipped() {
    for key in ["u=3", "o=sometimes", "w=-2", "w=soon", "a=explode"] {
        let mut session = Session::new(10, 4);
        let stats = send(&mut session, &[&format!("99;{key};Hi")]);
        assert_eq!(stats.unhandled_sequences, 1, "{key}");
        let notification = only(&session);
        assert_eq!(notification.urgency, None, "{key}");
        assert_eq!(notification.occasion, NotificationOccasion::Always, "{key}");
        assert_eq!(notification.expiry, NotificationExpiry::Platform, "{key}");
    }
    // A known action next to an unknown one still applies.
    let mut session = Session::new(10, 4);
    send(&mut session, &["99;a=explode,report;Hi"]);
    assert!(only(&session).report_activation);
}

#[test]
fn a_name_that_is_not_base64_text_is_malformed_and_one_too_long_is_skipped() {
    let invalid_utf8 = base64(&[0xff, 0xfe]);
    for value in ["!!!", invalid_utf8.as_str()] {
        let mut session = Session::new(10, 4);
        let stats = send(&mut session, &[&format!("99;f={value};Hi")]);
        assert_eq!(
            (stats.malformed_sequences, stats.unhandled_sequences),
            (1, 0)
        );
        assert_eq!(session.batch.str(only(&session).app_name), "");
    }
    let mut session = Session::new(10, 4);
    let longest = "x".repeat(NOTIFICATION_NAME_MAX);
    let stats = send(
        &mut session,
        &[&format!("99;f={};Hi", base64(longest.as_bytes()))],
    );
    assert_eq!(stats.unhandled_sequences, 0);
    assert_eq!(session.batch.str(only(&session).app_name), longest);
    let stats = send(
        &mut session,
        &[&format!(
            "99;f={};Hi",
            base64(format!("{longest}x").as_bytes())
        )],
    );
    assert_eq!(stats.unhandled_sequences, 1);
    assert_eq!(session.batch.str(only(&session).app_name), "");
}

#[test]
fn a_list_holds_each_name_once_and_no_more_than_its_cap() {
    let mut session = Session::new(10, 4);
    let kind = |index: usize| base64(format!("kind-{index}").as_bytes());
    let mut metadata = format!("t={}:t={}", kind(0), kind(0));
    for index in 1..NOTIFICATION_LIST_MAX {
        metadata.push_str(&format!(":t={}", kind(index)));
    }
    let stats = send(&mut session, &[&format!("99;{metadata};Hi")]);
    assert_eq!(stats.unhandled_sequences, 0);
    let kinds = texts(&session, only(&session).kinds);
    assert_eq!(kinds.len(), NOTIFICATION_LIST_MAX);
    assert_eq!(kinds[0], "kind-0");
    assert_eq!(kinds[1], "kind-1");
    // One more than the list holds.
    let stats = send(
        &mut session,
        &[&format!(
            "99;{metadata}:t={};Hi",
            kind(NOTIFICATION_LIST_MAX)
        )],
    );
    assert_eq!(stats.unhandled_sequences, 1);
    assert_eq!(
        texts(&session, only(&session).kinds).len(),
        NOTIFICATION_LIST_MAX
    );
}

// ---- OSC 99: buttons and icons ----

#[test]
fn buttons_are_split_on_the_line_separator() {
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &[
            "99;i=1:d=0;Title",
            "99;i=1:d=0:p=buttons;Yes\u{2028}N",
            "99;i=1:p=buttons;o\u{2028}\u{2028}Later\u{2028}",
        ],
    );
    assert_eq!(stats.unhandled_sequences, 0);
    assert_eq!(
        texts(&session, only(&session).buttons),
        ["Yes", "No", "Later"]
    );
    let labels = base64("OK\u{2028}Cancel".as_bytes());
    send(
        &mut session,
        &[
            "99;i=2:d=0;Title",
            &format!("99;i=2:p=buttons:e=1;{labels}"),
        ],
    );
    assert_eq!(texts(&session, only(&session).buttons), ["OK", "Cancel"]);
}

#[test]
fn an_icon_arrives_as_the_bytes_that_were_sent() {
    let image = [0x89, b'P', b'N', b'G', 0, 1, 2, 0xff, 0x1b, 0x07];
    let encoded = base64(&image);
    let (first, second) = encoded.split_at(5);
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &[
            &format!("99;i=1:d=0:p=icon:e=1:g=key-1;{first}"),
            &format!("99;i=1:d=0:p=icon:e=1;{second}"),
            "99;i=1;Title",
        ],
    );
    assert_eq!(
        (stats.unhandled_sequences, stats.malformed_sequences),
        (0, 0)
    );
    let notification = only(&session);
    assert_eq!(session.batch.bytes(notification.icon), image);
    assert_eq!(session.batch.str(notification.icon_id), "key-1");
}

#[test]
fn icon_data_that_is_not_base64_is_not_taken() {
    let mut session = Session::new(10, 4);
    // Binary data has to be encoded: without `e=1` it is not read at all.
    let stats = send(
        &mut session,
        &["99;i=1:d=0:p=icon;rawbytes", "99;i=1;Title"],
    );
    assert_eq!(
        (stats.unhandled_sequences, stats.malformed_sequences),
        (1, 0)
    );
    assert!(session.batch.bytes(only(&session).icon).is_empty());
    let stats = send(
        &mut session,
        &["99;i=1:d=0:p=icon:e=1;!!!!", "99;i=1;Title"],
    );
    assert_eq!(
        (stats.unhandled_sequences, stats.malformed_sequences),
        (0, 1)
    );
    assert!(session.batch.bytes(only(&session).icon).is_empty());
}

#[test]
fn an_icon_past_its_cap_is_dropped_whole() {
    let chunk = base64(&[7u8; 1500]);
    let mut session = Session::new(10, 4);
    let mut refused = 0;
    for _ in 0..=NOTIFICATION_ICON_MAX / 1500 {
        refused +=
            send(&mut session, &[&format!("99;i=1:d=0:p=icon:e=1;{chunk}")]).unhandled_sequences;
    }
    assert_eq!(refused, 1);
    // Chunks that follow are skipped, and the notification comes without an icon.
    let stats = send(&mut session, &[&format!("99;i=1:d=0:p=icon:e=1;{chunk}")]);
    assert_eq!(stats.unhandled_sequences, 1);
    send(&mut session, &["99;i=1;Title"]);
    assert!(session.batch.bytes(only(&session).icon).is_empty());
}

#[test]
fn a_notification_needs_text_whatever_else_it_has() {
    let mut session = Session::new(10, 4);
    for body in ["99;p=buttons;Yes", "99;p=icon:e=1;AAAA", "99;p=video;data"] {
        let stats = send(&mut session, &[body]);
        assert!(shown(&session).is_empty(), "{body:?}");
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
    }
    // A payload type the engine does not know is skipped; the notification still ends.
    let stats = send(&mut session, &["99;i=1:d=0;Title", "99;i=1:p=video;data"]);
    assert_eq!(shown(&session), [pair("Title", "")]);
    assert_eq!(stats.unhandled_sequences, 1);
}

// ---- OSC 99: close and alive ----

#[test]
fn close_asks_the_embedder_to_close_a_notification() {
    let mut session = Session::new(10, 4);
    let stats = send(
        &mut session,
        &[
            "99;i=1:d=0;Title",
            "99;i=n-1:p=close;",
            "99;i=1:p=body;Body",
        ],
    );
    let closes: Vec<&str> = session
        .batch
        .iter()
        .filter_map(|event| match event {
            VtEvent::NotificationClose { id } => Some(session.batch.str(*id)),
            _ => None,
        })
        .collect();
    assert_eq!(closes, ["n-1"]);
    // The notification that was waiting is untouched.
    assert_eq!(shown(&session), [pair("Title", "Body")]);
    assert_eq!(stats.unhandled_sequences, 0);
    assert_eq!(session.replies(), "");
    // Without an id there is nothing to close.
    let stats = send(&mut session, &["99;p=close;"]);
    assert_eq!(session.batch.events(), [VtEvent::Repaint]);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn alive_asks_the_embedder_which_notifications_are_shown() {
    let mut session = Session::new(10, 4);
    for (body, expected) in [("99;i=q1:p=alive;", "q1"), ("99;p=alive;", "")] {
        let stats = send(&mut session, &[body]);
        let VtEvent::NotificationAliveQuery { id } = &session.batch.events()[0] else {
            panic!("an alive query");
        };
        assert_eq!(session.batch.str(*id), expected);
        // Only the embedder knows the answer.
        assert_eq!(session.replies(), "");
        assert_eq!(stats.unhandled_sequences, 0);
    }
}

// ---- OSC 99: what the query answers ----

fn answer(support: NotificationSupport) -> String {
    let mut session = Session::with(Config {
        notifications: support,
        ..Config::default()
    });
    session.feed(b"\x1b]99;i=q:p=?;\x1b\\");
    let reply = session.replies();
    reply
        .strip_prefix("\x1b]99;i=q:p=?;")
        .and_then(|rest| rest.strip_suffix("\x1b\\"))
        .expect("an answer")
        .to_owned()
}

#[test]
fn the_query_answer_lists_what_the_embedder_declared() {
    assert_eq!(
        answer(NotificationSupport::default()),
        "o=always:p=title,body,?"
    );
    assert_eq!(
        answer(NotificationSupport {
            report: true,
            urgency: true,
            ..NotificationSupport::default()
        }),
        "a=report:o=always:p=title,body,?:u=0,1,2"
    );
    let everything = NotificationSupport {
        focus: true,
        report: true,
        close_events: true,
        unfocused: true,
        invisible: true,
        icons: true,
        buttons: true,
        close: true,
        alive: true,
        sounds: vec![
            Cow::Borrowed("system"),
            Cow::Borrowed("silent"),
            Cow::Borrowed("no:good"),
            Cow::Borrowed(""),
        ],
        urgency: true,
        expiry: true,
    };
    assert_eq!(
        answer(everything),
        "a=focus,report:c=1:o=always,unfocused,invisible:\
         p=title,body,?,close,icon,alive,buttons:s=system,silent:u=0,1,2:w=1"
    );
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
