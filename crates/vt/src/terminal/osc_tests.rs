//! Tests of OSC routing, the built-in handlers, colour overrides and the title stack.

use super::color::{COLOR_COUNT, parse_color};
use super::terminal_tests::Session;
use super::*;
use crate::cell::Rgb;
use crate::events::{ClipboardKind, Progress, ShellMark, VtEvent};
use crate::parser::StringTerm;

/// Feeds `ESC ] body BEL` into a fresh 10x4 terminal.
fn run(body: &str) -> (Session, FeedStats) {
    run_with(Config::default(), body)
}

fn run_with(config: Config, body: &str) -> (Session, FeedStats) {
    let mut session = Session::sized(10, 4, config);
    let stats = session.feed(format!("\x1b]{body}\x07").as_bytes());
    (session, stats)
}

fn events_of(session: &Session) -> Vec<&VtEvent> {
    session
        .batch
        .iter()
        .filter(|event| **event != VtEvent::Repaint)
        .collect()
}

fn text(session: &Session, span: crate::events::StrSpan) -> String {
    session.batch.str(span).to_owned()
}

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

// ---- Routes ----

#[test]
fn defaults_run_builtins_and_drop_the_rest() {
    let routes = OscRoutes::new();
    assert_eq!(OscRoutes::BUILTIN.len(), 20);
    assert!(OscRoutes::BUILTIN.windows(2).all(|pair| pair[0] < pair[1]));
    for code in OscRoutes::BUILTIN {
        assert!(OscRoutes::has_builtin(code));
        assert_eq!(routes.get(code), OscRoute::Builtin, "{code}");
    }
    for code in [3, 5, 6, 99, 2047, 2048, 7501, u32::MAX] {
        assert!(!OscRoutes::has_builtin(code));
        assert_eq!(routes.get(code), OscRoute::Drop, "{code}");
    }
    assert_eq!(routes.overrides().count(), 0);
}

#[test]
fn each_route_reads_back_for_a_builtin_and_a_private_number() {
    let mut routes = OscRoutes::new();
    for route in [
        OscRoute::BuiltinAndForward,
        OscRoute::Forward,
        OscRoute::Drop,
        OscRoute::Builtin,
    ] {
        routes.route(52, route);
        assert_eq!(routes.get(52), route);
    }
    routes.route(7501, OscRoute::Forward);
    assert_eq!(routes.get(7501), OscRoute::Forward);
    routes.route(7501, OscRoute::Drop);
    assert_eq!(routes.get(7501), OscRoute::Drop);
}

#[test]
fn a_builtin_route_without_a_builtin_reads_back_as_drop_or_forward() {
    let mut routes = OscRoutes::new();
    routes.route(7501, OscRoute::Builtin);
    assert_eq!(routes.get(7501), OscRoute::Drop);
    routes.route(7501, OscRoute::BuiltinAndForward);
    assert_eq!(routes.get(7501), OscRoute::Forward);
}

#[test]
fn equal_behaviour_is_equal_tables() {
    let fresh = OscRoutes::new();
    let mut routes = OscRoutes::new();
    routes
        .route(7501, OscRoute::Forward)
        .route(7501, OscRoute::Drop);
    assert_eq!(routes, fresh);
    routes.route(52, OscRoute::BuiltinAndForward);
    assert_ne!(routes, fresh);
    routes.route(52, OscRoute::Builtin);
    assert_eq!(routes, fresh);
    routes
        .route(7501, OscRoute::Builtin)
        .route(9000, OscRoute::Drop);
    assert_eq!(routes, fresh);
    let mut a = OscRoutes::new();
    let mut b = OscRoutes::new();
    a.route(7501, OscRoute::BuiltinAndForward);
    b.route(7501, OscRoute::Forward);
    assert_eq!(a, b);
}

#[test]
fn route_all_is_route_per_code() {
    let mut all = OscRoutes::new();
    all.route_all(&[2, 52, 7501, 9000], OscRoute::Forward);
    let mut each = OscRoutes::new();
    for code in [2, 52, 7501, 9000] {
        each.route(code, OscRoute::Forward);
    }
    assert_eq!(all, each);
}

#[test]
fn large_is_per_number_and_orthogonal_to_the_route() {
    let mut routes = OscRoutes::new();
    assert!(!routes.allows_large(52));
    routes.large(52, true).large(9000, true);
    assert!(routes.allows_large(52) && routes.allows_large(9000));
    assert!(!routes.allows_large(0));
    routes.large(52, false);
    assert!(!routes.allows_large(52));
    assert_eq!(routes.get(52), OscRoute::Builtin);
}

#[test]
fn numbers_above_the_bitmap_are_kept_in_the_spill() {
    let mut routes = OscRoutes::new();
    routes.route(2047, OscRoute::Forward);
    routes.route(2048, OscRoute::Forward);
    routes.route(100_000, OscRoute::Forward);
    routes.route(100_000, OscRoute::Drop);
    routes.large(5000, true);
    assert_eq!(routes.get(2047), OscRoute::Forward);
    assert_eq!(routes.get(2048), OscRoute::Forward);
    assert_eq!(routes.get(100_000), OscRoute::Drop);
    assert!(routes.allows_large(5000));
}

#[test]
fn overrides_list_exactly_the_non_default_numbers_ascending() {
    let mut routes = OscRoutes::new();
    routes
        .route(7501, OscRoute::Forward)
        .route(52, OscRoute::Drop)
        .route(2, OscRoute::BuiltinAndForward)
        .route(5000, OscRoute::Forward)
        .route(3000, OscRoute::Forward)
        .route(9, OscRoute::Builtin)
        .route(4000, OscRoute::Drop)
        .route(1, OscRoute::Forward);
    let listed: Vec<(u32, OscRoute)> = routes.overrides().collect();
    assert_eq!(
        listed,
        [
            (1, OscRoute::Forward),
            (2, OscRoute::BuiltinAndForward),
            (52, OscRoute::Drop),
            (3000, OscRoute::Forward),
            (5000, OscRoute::Forward),
            (7501, OscRoute::Forward),
        ]
    );
}

#[test]
fn builtin_and_forward_pushes_the_typed_event_before_the_osc() {
    let mut config = Config::default();
    config.osc_routes.route(2, OscRoute::BuiltinAndForward);
    let (session, stats) = run_with(config, "2;hi");
    let events = events_of(&session);
    assert!(matches!(events[0], VtEvent::Title(span) if text(&session, *span) == "hi"));
    let VtEvent::Osc {
        code,
        params,
        terminator,
        truncated,
    } = events[1]
    else {
        panic!("Osc second");
    };
    assert_eq!(
        (*code, *terminator, *truncated),
        (2, StringTerm::Bel, false)
    );
    let read: Vec<&[u8]> = session.batch.params(*params).collect();
    assert_eq!(read, [&b"2"[..], b"hi"]);
    assert_eq!(session.term.title(), Some("hi"));
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn forward_skips_the_builtin_and_does_not_count() {
    let mut config = Config::default();
    config.osc_routes.route(2, OscRoute::Forward);
    let (session, stats) = run_with(config, "2;hi");
    assert_eq!(events_of(&session).len(), 1);
    assert!(matches!(
        events_of(&session)[0],
        VtEvent::Osc { code: 2, .. }
    ));
    assert_eq!(session.term.title(), None);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn drop_skips_everything_and_counts() {
    let mut config = Config::default();
    config.osc_routes.route(2, OscRoute::Drop);
    let (session, stats) = run_with(config, "2;hi");
    assert!(events_of(&session).is_empty());
    assert_eq!(stats.unhandled_sequences, 1);
    let (session, stats) = run("3;x");
    assert!(events_of(&session).is_empty());
    assert_eq!(stats.unhandled_sequences, 1);
}

#[test]
fn builtin_only_forwards_nothing() {
    let (session, _) = run("2;hi");
    assert!(
        !session
            .batch
            .iter()
            .any(|event| matches!(event, VtEvent::Osc { .. }))
    );
}

#[test]
fn a_forwarded_private_number_carries_its_parameters_and_terminator() {
    let mut config = Config::default();
    config.osc_routes.route(7501, OscRoute::Forward);
    let mut session = Session::sized(10, 4, config);
    session.feed(b"\x1b]7501;a;;b\x1b\\");
    let VtEvent::Osc {
        params, terminator, ..
    } = events_of(&session)[0]
    else {
        panic!("Osc");
    };
    assert_eq!(*terminator, StringTerm::St);
    let read: Vec<&[u8]> = session.batch.params(*params).collect();
    assert_eq!(read, [&b"7501"[..], b"a", b"", b"b"]);
}

#[test]
fn large_payloads_need_the_opt_in_and_a_live_route() {
    let payload = "x".repeat(5000);
    let stream = format!("\x1b]7501;{payload}\x07");
    let mut config = Config::default();
    config.osc_routes.route(7501, OscRoute::Forward);
    let mut session = Session::sized(10, 4, config.clone());
    assert_eq!(session.feed(stream.as_bytes()).truncated_osc, 1);

    config.osc_routes.large(7501, true);
    let mut session = Session::sized(10, 4, config.clone());
    assert_eq!(session.feed(stream.as_bytes()).truncated_osc, 0);

    config.osc_routes.route(7501, OscRoute::Drop);
    let mut session = Session::sized(10, 4, config);
    let stats = session.feed(stream.as_bytes());
    assert_eq!(
        stats.truncated_osc, 1,
        "a dropped number gets no large buffer"
    );
    assert_eq!(stats.unhandled_sequences, 1);
}

#[test]
fn truncated_osc_counts_before_routing() {
    let long = "t".repeat(3000);
    let (session, stats) = run(&format!("0;{long}"));
    assert_eq!(stats.truncated_osc, 1);
    assert_eq!(session.term.title().map(str::len), Some(2047));
    let (_, stats) = run(&format!("3;{long}"));
    assert_eq!((stats.truncated_osc, stats.unhandled_sequences), (1, 1));
}

#[test]
fn zero_padded_codes_are_the_same_number() {
    let (session, _) = run("002;padded");
    assert_eq!(session.term.title(), Some("padded"));
    let (session, _) = run("007;file:///x");
    assert!(
        session
            .batch
            .iter()
            .any(|event| matches!(event, VtEvent::Cwd { .. }))
    );
}

#[test]
fn an_osc_without_a_number_is_unhandled() {
    for body in [";x", "x;y", "", "99999999999;x"] {
        let (session, stats) = run(body);
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
        assert!(events_of(&session).is_empty(), "{body:?}");
    }
}

// ---- Titles ----

#[test]
fn osc_0_and_2_set_the_title() {
    for code in [0, 2] {
        let (session, stats) = run(&format!("{code};  my title \t"));
        assert_eq!(session.term.title(), Some("my title"));
        assert!(matches!(
            events_of(&session)[0],
            VtEvent::Title(span) if text(&session, *span) == "my title"
        ));
        assert_eq!(stats.unhandled_sequences, 0);
    }
    let (session, _) = run("2;a;b;c");
    assert_eq!(session.term.title(), Some("a;b;c"));
}

#[test]
fn an_empty_title_resets() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b]2;x\x07");
    session.feed(b"\x1b]0;  \x07");
    assert_eq!(session.term.title(), None);
    assert_eq!(events_of(&session), [&VtEvent::TitleReset]);
}

#[test]
fn a_title_without_text_parameter_is_unhandled() {
    let (session, stats) = run("0");
    assert_eq!(stats.unhandled_sequences, 1);
    assert!(events_of(&session).is_empty());
}

#[test]
fn title_text_skips_parameters_that_are_not_utf8() {
    let mut session = Session::new(10, 4);
    let stats = session.feed(b"\x1b]2;a;\xffb;c\x07");
    assert_eq!(session.term.title(), Some("a;c"));
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn osc_1_sets_the_icon_name_only() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b]2;win\x07");
    session.feed(b"\x1b]1; icon \x07");
    assert!(matches!(
        events_of(&session)[0],
        VtEvent::IconName(span) if text(&session, *span) == "icon"
    ));
    assert_eq!(session.term.title(), Some("win"));
    session.feed(b"\x1b]1;\x07");
    assert!(matches!(
        events_of(&session)[0],
        VtEvent::IconName(span) if text(&session, *span).is_empty()
    ));
    assert_eq!(session.feed(b"\x1b]1\x07").unhandled_sequences, 1);
}

#[test]
fn the_title_stack_pushes_pops_and_caps_at_sixteen() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b]2;a\x07\x1b[22t\x1b]2;b\x07");
    assert_eq!(session.term.title_depth(), 1);
    session.feed(b"\x1b[23t");
    assert_eq!(session.term.title(), Some("a"));
    assert!(matches!(
        events_of(&session)[0],
        VtEvent::Title(span) if text(&session, *span) == "a"
    ));
    assert_eq!(session.term.title_depth(), 0);
    session.feed(b"\x1b[23t");
    assert!(
        events_of(&session).is_empty(),
        "an empty stack pops nothing"
    );

    session.feed(b"\x1b]0;\x07\x1b[22t\x1b]2;x\x07");
    session.feed(b"\x1b[23t");
    assert_eq!(session.term.title(), None);
    assert_eq!(events_of(&session), [&VtEvent::TitleReset]);

    for n in 0..40 {
        session.feed(format!("\x1b]2;t{n}\x07\x1b[22t").as_bytes());
    }
    assert_eq!(session.term.title_depth(), 16);
    session.feed(b"\x1b[23t");
    assert_eq!(session.term.title(), Some("t39"));
    for _ in 0..15 {
        session.feed(b"\x1b[23t");
    }
    assert_eq!(
        session.term.title(),
        Some("t24"),
        "the oldest 24 were dropped"
    );
    assert_eq!(session.term.title_depth(), 0);
}

#[test]
fn window_operations_other_than_the_known_are_unhandled() {
    let mut session = Session::new(10, 4);
    assert_eq!(session.feed(b"\x1b[7t").unhandled_sequences, 1);
    assert_eq!(session.feed(b"\x1b[22;2t").unhandled_sequences, 0);
    assert_eq!(session.term.title_depth(), 1);
}

// ---- OSC 7 ----

fn cwd(body: &str) -> Option<(String, String)> {
    let (session, _) = run(body);
    events_of(&session).iter().find_map(|event| match event {
        VtEvent::Cwd { host, path } => Some((text(&session, *host), text(&session, *path))),
        _ => None,
    })
}

#[test]
fn osc_7_splits_host_and_decodes_the_path() {
    let pair = |host: &str, path: &str| Some((host.to_owned(), path.to_owned()));
    assert_eq!(cwd("7;file://host/a/b%20c"), pair("host", "/a/b c"));
    assert_eq!(cwd("7;file:///home/me"), pair("", "/home/me"));
    assert_eq!(cwd("7;file://host"), pair("host", ""));
    assert_eq!(cwd("7;file://host/"), pair("host", "/"));
    assert_eq!(cwd("7;/plain/path"), pair("", "/plain/path"));
    assert_eq!(cwd("7;"), pair("", ""));
    assert_eq!(cwd("7;file:///C:/Users"), pair("", "C:/Users"));
    assert_eq!(cwd("7;file:///C:"), pair("", "C:"));
    assert_eq!(cwd("7;file:///c:\\Users"), pair("", "c:\\Users"));
    assert_eq!(cwd("7;file:///CC:/x"), pair("", "/CC:/x"));
    assert_eq!(cwd("7;file:///a%2Fb%zz"), pair("", "/a/b%zz"));
    assert_eq!(cwd("7;file:///%E2%82%AC"), pair("", "/\u{20AC}"));
    assert_eq!(cwd("7;file:///%FF"), pair("", "/\u{FFFD}"));
}

#[test]
fn osc_7_rejoins_semicolons_in_the_path() {
    assert_eq!(
        cwd("7;file:///a;b;c"),
        Some((String::new(), "/a;b;c".to_owned()))
    );
}

#[test]
fn osc_7_rejects_bad_utf8_and_truncated_payloads() {
    let mut session = Session::new(10, 4);
    let stats = session.feed(b"\x1b]7;file:///\xff\x07");
    assert_eq!(stats.unhandled_sequences, 1);
    assert!(events_of(&session).is_empty());
    let long = format!("7;file:///{}", "a".repeat(3000));
    let (session, stats) = run(&long);
    assert_eq!((stats.truncated_osc, stats.unhandled_sequences), (1, 1));
    assert!(events_of(&session).is_empty());
}

// ---- OSC 8 ----

#[test]
fn hyperlinks_attach_to_printed_cells_and_close() {
    let mut session = Session::new(20, 2);
    session.feed(b"\x1b]8;id=a:foo=1;http://x.test/a\x07ab\x1b]8;;\x07c");
    let screen = session.term.screen();
    let row = screen.row(screen.row_of_index(0));
    let link = |col| {
        let extras = session
            .term
            .interner()
            .resolve_extras(row.cell(col).extras_id());
        extras.hyperlink
    };
    let first = link(0).expect("a linked");
    assert_eq!(link(1), Some(first));
    assert_eq!(link(2), None, "closed before c");
    let resolved = session
        .term
        .interner()
        .hyperlinks
        .resolve(first)
        .expect("stored");
    assert_eq!(&*resolved.id, "a");
    assert_eq!(&*resolved.uri, "http://x.test/a");
    assert!(!resolved.implicit);
}

#[test]
fn hyperlinks_without_an_id_are_implicit_and_deduplicated() {
    let mut session = Session::new(20, 2);
    session.feed(b"\x1b]8;;http://a;b\x07x\x1b]8;;\x07\x1b]8;;http://a;b\x07y");
    assert_eq!(session.term.interner().hyperlinks.len(), 1);
    let link = session
        .term
        .interner()
        .hyperlinks
        .resolve(crate::intern::HyperlinkId(0))
        .expect("stored");
    assert_eq!(&*link.uri, "http://a;b");
    assert!(link.implicit);
}

#[test]
fn a_hyperlink_keeps_the_style_and_a_style_keeps_the_hyperlink() {
    let mut session = Session::new(20, 2);
    session.feed(b"\x1b[1m\x1b]8;;http://x\x07\x1b[3ma");
    let template = session.term.template();
    assert_ne!(template.extras_id(), crate::intern::ExtrasId::NONE);
    assert_eq!(
        session.term.style().attrs,
        crate::cell::Attrs::BOLD | crate::cell::Attrs::ITALIC
    );
}

#[test]
fn osc_8_with_too_few_parameters_is_unhandled() {
    let (_, stats) = run("8;");
    assert_eq!(stats.unhandled_sequences, 1);
    let (_, stats) = run("8");
    assert_eq!(stats.unhandled_sequences, 1);
    let (_, stats) = run("8;;");
    assert_eq!(stats.unhandled_sequences, 0);
}

// ---- OSC 9 ----

#[test]
fn osc_9_is_a_notification_with_an_empty_title() {
    let (session, stats) = run("9;build done");
    let VtEvent::Notification { title, body } = events_of(&session)[0] else {
        panic!("notification");
    };
    assert_eq!(text(&session, *title), "");
    assert_eq!(text(&session, *body), "build done");
    assert_eq!(stats.unhandled_sequences, 0);
    let (session, _) = run("9;a;b");
    let VtEvent::Notification { body, .. } = events_of(&session)[0] else {
        panic!("notification");
    };
    assert_eq!(text(&session, *body), "a;b");
}

#[test]
fn osc_9_rejects_nothing_to_say() {
    for body in ["9", "9;"] {
        let (session, stats) = run(body);
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
        assert!(events_of(&session).is_empty());
    }
}

fn progress(body: &str) -> Option<Progress> {
    let (session, _) = run(body);
    events_of(&session).iter().find_map(|event| match event {
        VtEvent::Progress(progress) => Some(*progress),
        _ => None,
    })
}

#[test]
fn osc_9_4_reports_progress() {
    assert_eq!(progress("9;4;0"), Some(Progress::Remove));
    assert_eq!(progress("9;4"), Some(Progress::Remove));
    assert_eq!(progress("9;4;1;50"), Some(Progress::Set(50)));
    assert_eq!(progress("9;4;1;200"), Some(Progress::Set(100)));
    assert_eq!(progress("9;4;1"), Some(Progress::Set(0)));
    assert_eq!(progress("9;4;2;30"), Some(Progress::Error(30)));
    assert_eq!(progress("9;4;3"), Some(Progress::Indeterminate));
    assert_eq!(progress("9;4;4;80"), Some(Progress::Paused(80)));
    for bad in ["9;4;5", "9;4;x", "9;4;1;y", "9;4;-1"] {
        let (session, stats) = run(bad);
        assert_eq!(stats.unhandled_sequences, 1, "{bad:?}");
        assert!(events_of(&session).is_empty(), "{bad:?}");
    }
    let (_, stats) = run(&format!("9;4;1;50;{}", "x".repeat(3000)));
    assert_eq!((stats.truncated_osc, stats.unhandled_sequences), (1, 1));
}

// ---- OSC 22, 50, 52, 133 ----

#[test]
fn osc_22_names_the_pointer() {
    let (session, _) = run("22;pointer");
    assert!(matches!(
        events_of(&session)[0],
        VtEvent::Pointer(span) if text(&session, *span) == "pointer"
    ));
    for body in ["22", "22;"] {
        assert_eq!(run(body).1.unhandled_sequences, 1);
    }
}

#[test]
fn osc_50_sets_the_cursor_shape_and_keeps_the_blink() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b[3 q");
    for (value, shape) in [
        (1, CursorShape::Beam),
        (2, CursorShape::Underline),
        (0, CursorShape::Block),
    ] {
        session.feed(format!("\x1b]50;CursorShape={value}\x07").as_bytes());
        assert_eq!(
            session.term.cursor_style(),
            CursorStyle {
                shape,
                blinking: true
            }
        );
        assert!(events_of(&session).contains(&&VtEvent::CursorStyleChanged));
    }
    session.feed(b"\x1b]50;CursorShape=0\x07");
    assert!(!events_of(&session).contains(&&VtEvent::CursorStyleChanged));
    for body in ["50;CursorShape=3", "50;Other=1", "50"] {
        let stats = session.feed(format!("\x1b]{body}\x07").as_bytes());
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
    }
}

#[test]
fn osc_52_stores_and_loads() {
    for (body, kind) in [
        ("52;c;Zm9v", ClipboardKind::Clipboard),
        ("52;;Zm9v", ClipboardKind::Clipboard),
        ("52;p;Zm9v", ClipboardKind::Primary),
        ("52;s;Zm9v", ClipboardKind::Secondary),
        ("52;pc;Zm9v", ClipboardKind::Primary),
    ] {
        let (session, stats) = run(body);
        let VtEvent::ClipboardStore {
            selection,
            text: span,
        } = events_of(&session)[0]
        else {
            panic!("store for {body}");
        };
        assert_eq!(*selection, kind, "{body}");
        assert_eq!(text(&session, *span), "foo");
        assert_eq!(stats.unhandled_sequences + stats.malformed_sequences, 0);
    }
    let (session, _) = run("52;c;");
    assert!(matches!(
        events_of(&session)[0],
        VtEvent::ClipboardStore { text: span, .. } if text(&session, *span).is_empty()
    ));
    let (session, _) = run("52;p;?");
    assert_eq!(
        events_of(&session),
        [&VtEvent::ClipboardLoad {
            selection: ClipboardKind::Primary
        }]
    );
}

#[test]
fn osc_52_rejections_and_malformed_payloads() {
    for body in ["52", "52;c", "52;x;Zm9v"] {
        let (session, stats) = run(body);
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
        assert_eq!(stats.malformed_sequences, 0, "{body:?}");
        assert!(events_of(&session).is_empty());
    }
    for payload in ["Zm9", "Zm=v", "Z===", "Zg==Zg==", "Zm9v!", "/w==", "!!!!"] {
        let (session, stats) = run(&format!("52;c;{payload}"));
        assert_eq!(stats.malformed_sequences, 1, "{payload:?}");
        assert_eq!(stats.unhandled_sequences, 0, "{payload:?}");
        assert!(events_of(&session).is_empty(), "{payload:?}");
    }
}

#[test]
fn osc_133_marks_and_the_pen_zone() {
    use crate::cell::Semantic;
    let mut session = Session::new(10, 4);
    for (mark, event, zone) in [
        ("A", ShellMark::PromptStart, Semantic::Prompt),
        ("B", ShellMark::PromptEnd, Semantic::Input),
        ("C", ShellMark::OutputStart, Semantic::Output),
        (
            "D",
            ShellMark::OutputEnd { exit_code: None },
            Semantic::None,
        ),
        (
            "D;3",
            ShellMark::OutputEnd { exit_code: Some(3) },
            Semantic::None,
        ),
        (
            "D;-1;aid=2",
            ShellMark::OutputEnd {
                exit_code: Some(-1),
            },
            Semantic::None,
        ),
        (
            "D;x",
            ShellMark::OutputEnd { exit_code: None },
            Semantic::None,
        ),
    ] {
        session.feed(format!("\x1b]133;{mark}\x07").as_bytes());
        assert_eq!(events_of(&session), [&VtEvent::ShellMark(event)], "{mark}");
        assert_eq!(session.term.template().semantic(), zone, "{mark}");
    }
    session.feed(b"\x1b]133;C\x07x");
    let screen = session.term.screen();
    assert_eq!(
        screen.row(screen.row_of_index(0)).cell(0).semantic(),
        Semantic::Output
    );
    for body in ["133;Z", "133", "133;"] {
        let stats = session.feed(format!("\x1b]{body}\x07").as_bytes());
        assert_eq!(stats.unhandled_sequences, 1, "{body:?}");
    }
}

// ---- Colours ----

#[test]
fn colour_keys_round_trip_through_their_index() {
    let keys = (0..=255)
        .map(ColorKey::Palette)
        .chain([
            ColorKey::Foreground,
            ColorKey::Background,
            ColorKey::Cursor,
            ColorKey::SelectionBackground,
            ColorKey::SelectionForeground,
        ])
        .collect::<Vec<_>>();
    for key in keys {
        assert_eq!(ColorKey::from_index(key.index()), Some(key));
    }
    assert_eq!(ColorKey::Foreground.index(), 256);
    assert_eq!(ColorKey::Cursor.index(), 258);
    assert_eq!(ColorKey::SelectionBackground.index(), 269);
    assert_eq!(ColorKey::SelectionForeground.index(), 270);
    for index in (259..=268).chain(COLOR_COUNT..COLOR_COUNT + 5) {
        assert_eq!(ColorKey::from_index(index), None, "{index}");
    }
    assert_eq!(ColorKey::from_index(usize::MAX), None);
}

#[test]
fn colour_override_iteration_is_ordered() {
    let mut overrides = ColorOverrides::default();
    assert_eq!(overrides.iter().count(), 0);
    assert!(overrides.set(ColorKey::SelectionForeground, rgb(1, 1, 1)));
    assert!(overrides.set(ColorKey::Palette(7), rgb(2, 2, 2)));
    assert!(overrides.set(ColorKey::Background, rgb(3, 3, 3)));
    assert!(
        !overrides.set(ColorKey::Background, rgb(3, 3, 3)),
        "no change"
    );
    let listed: Vec<(usize, Rgb)> = overrides.iter().collect();
    assert_eq!(
        listed,
        [(7, rgb(2, 2, 2)), (257, rgb(3, 3, 3)), (270, rgb(1, 1, 1))]
    );
    assert!(overrides.reset(ColorKey::Background));
    assert!(!overrides.reset(ColorKey::Background));
    assert!(overrides.reset_palette());
    assert!(!overrides.reset_palette());
    assert!(overrides.reset_all());
    assert_eq!(overrides.get(ColorKey::SelectionForeground), None);
}

#[test]
fn colour_specs() {
    for (spec, expected) in [
        ("#f00", Some(rgb(0xf0, 0, 0))),
        ("#FFA500", Some(rgb(255, 165, 0))),
        ("#ffa5000000ff", Some(rgb(255, 0, 0))),
        ("#abcdefghi", None),
        ("#fff000fff", Some(rgb(255, 0, 255))),
        ("#123456789abc", Some(rgb(0x12, 0x56, 0x9a))),
        ("#ff", None),
        ("#", None),
        ("#fffg", None),
        ("#ggg", None),
        ("#1234567890123", None),
        ("rgb:f/8/0", Some(rgb(255, 136, 0))),
        ("rgb:ff/80/00", Some(rgb(255, 128, 0))),
        ("rgb:ffff/8000/0", Some(rgb(255, 127, 0))),
        ("rgb:FFF/000/000", Some(rgb(255, 0, 0))),
        ("rgb:ff/ff", None),
        ("rgb:ff/ff/ff/ff", None),
        ("rgb:fffff/0/0", None),
        ("rgb:/0/0", None),
        ("rgb:g/0/0", None),
        ("RGB:ff/ff/ff", None),
        ("rgbi:1/1/1", None),
        ("red", None),
        ("", None),
    ] {
        assert_eq!(parse_color(spec.as_bytes()), expected, "{spec:?}");
    }
}

#[test]
fn color_reply_bytes() {
    assert_eq!(
        ColorKey::Background.color_reply(rgb(30, 30, 30), StringTerm::Bel),
        b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07"
    );
    assert_eq!(
        ColorKey::Palette(1).color_reply(rgb(205, 0, 0), StringTerm::St),
        b"\x1b]4;1;rgb:cdcd/0000/0000\x1b\\"
    );
    assert_eq!(ColorKey::Palette(255).query_prefix(), "4;255");
    assert_eq!(ColorKey::SelectionBackground.query_prefix(), "17");
    assert_eq!(ColorKey::SelectionForeground.query_prefix(), "19");
    assert_eq!(ColorKey::Foreground.query_prefix(), "10");
    assert_eq!(ColorKey::Cursor.query_prefix(), "12");
}

#[test]
fn osc_4_sets_queries_and_rejects_per_pair() {
    let (session, stats) = run("4;1;rgb:ff/00/00;2;#00ff00");
    assert_eq!(
        session.term.color(ColorKey::Palette(1)),
        Some(rgb(255, 0, 0))
    );
    assert_eq!(
        session.term.color(ColorKey::Palette(2)),
        Some(rgb(0, 255, 0))
    );
    assert_eq!(stats.unhandled_sequences, 0);

    let (session, _) = run("4;1;?;2;?");
    assert_eq!(
        events_of(&session),
        [
            &VtEvent::ColorQuery {
                key: ColorKey::Palette(1),
                terminator: StringTerm::Bel
            },
            &VtEvent::ColorQuery {
                key: ColorKey::Palette(2),
                terminator: StringTerm::Bel
            }
        ]
    );

    let (session, stats) = run("4;256;#fff;x;#fff;3;red;4;#123");
    assert_eq!(stats.unhandled_sequences, 3);
    assert_eq!(
        session.term.color(ColorKey::Palette(4)),
        Some(rgb(0x10, 0x20, 0x30))
    );

    let (session, stats) = run("4;1;#fff;2");
    assert_eq!(
        stats.unhandled_sequences, 0,
        "a trailing odd parameter is ignored"
    );
    assert!(session.term.color(ColorKey::Palette(1)).is_some());
    assert_eq!(run("4").1.unhandled_sequences, 1);
    assert_eq!(run("4;1").1.unhandled_sequences, 1);
}

#[test]
fn osc_4_with_twelve_pairs_resplits_the_sixteenth_parameter() {
    let pairs: Vec<String> = (0..12).map(|n| format!("{n};#0{n:x}0")).collect();
    let (session, stats) = run(&format!("4;{}", pairs.join(";")));
    assert_eq!(stats.unhandled_sequences, 0);
    for n in 0..12u8 {
        assert_eq!(
            session.term.color(ColorKey::Palette(n)),
            Some(rgb(0, n << 4, 0)),
            "{n}"
        );
    }
}

#[test]
fn dynamic_colours_set_query_and_advance() {
    let (session, _) = run("10;rgb:ff/00/00");
    assert_eq!(
        session.term.color(ColorKey::Foreground),
        Some(rgb(255, 0, 0))
    );
    let (session, _) = run("10;#fff;#000;#123");
    assert_eq!(
        session.term.color(ColorKey::Foreground),
        Some(rgb(240, 240, 240))
    );
    assert_eq!(session.term.color(ColorKey::Background), Some(rgb(0, 0, 0)));
    assert_eq!(
        session.term.color(ColorKey::Cursor),
        Some(rgb(0x10, 0x20, 0x30))
    );
    let (session, _) = run("11;?;?");
    assert_eq!(
        events_of(&session),
        [
            &VtEvent::ColorQuery {
                key: ColorKey::Background,
                terminator: StringTerm::Bel
            },
            &VtEvent::ColorQuery {
                key: ColorKey::Cursor,
                terminator: StringTerm::Bel
            }
        ]
    );
    let (session, stats) = run("12;?;?");
    assert_eq!(events_of(&session).len(), 1);
    assert_eq!(
        stats.unhandled_sequences, 1,
        "an argument past 12 stops the walk"
    );
    let (_, stats) = run("10");
    assert_eq!(stats.unhandled_sequences, 1);
    let (_, stats) = run("10;red");
    assert_eq!(stats.unhandled_sequences, 1);
}

#[test]
fn a_query_terminated_by_st_echoes_it() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b]11;?\x1b\\");
    assert_eq!(
        events_of(&session),
        [&VtEvent::ColorQuery {
            key: ColorKey::Background,
            terminator: StringTerm::St
        }]
    );
}

#[test]
fn selection_colours_set_query_and_count_extra_parameters() {
    let (session, stats) = run("17;rgb:11/22/33");
    assert_eq!(
        session.term.color(ColorKey::SelectionBackground),
        Some(rgb(0x11, 0x22, 0x33))
    );
    assert_eq!(stats.unhandled_sequences, 0);
    let (session, _) = run("19;?");
    assert_eq!(
        events_of(&session),
        [&VtEvent::ColorQuery {
            key: ColorKey::SelectionForeground,
            terminator: StringTerm::Bel
        }]
    );
    for body in ["17", "17;", "19"] {
        assert_eq!(run(body).1.unhandled_sequences, 1, "{body:?}");
    }
    let (session, stats) = run("17;#fff;extra");
    assert_eq!(stats.unhandled_sequences, 1);
    assert!(session.term.color(ColorKey::SelectionBackground).is_some());
}

#[test]
fn colour_resets() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b]4;1;#fff;2;#fff;3;#fff\x07\x1b]10;#fff;#000;#111\x07\x1b]17;#fff\x07");
    session.feed(b"\x1b]104;1;3\x07");
    assert_eq!(session.term.color(ColorKey::Palette(1)), None);
    assert!(session.term.color(ColorKey::Palette(2)).is_some());
    assert_eq!(session.term.color(ColorKey::Palette(3)), None);
    assert_eq!(session.feed(b"\x1b]104;999\x07").unhandled_sequences, 1);
    session.feed(b"\x1b]104\x07");
    assert_eq!(session.term.color(ColorKey::Palette(2)), None);
    session.feed(b"\x1b]4;5;#fff\x07\x1b]104;\x07");
    assert_eq!(session.term.color(ColorKey::Palette(5)), None);
    assert!(
        session.term.color(ColorKey::Foreground).is_some(),
        "104 leaves the others"
    );

    session.feed(b"\x1b]110;ignored\x07");
    assert_eq!(session.term.color(ColorKey::Foreground), None);
    session.feed(b"\x1b]111\x07\x1b]112\x07");
    assert_eq!(session.term.color(ColorKey::Background), None);
    assert_eq!(session.term.color(ColorKey::Cursor), None);
    assert!(
        session.term.color(ColorKey::SelectionBackground).is_some(),
        "only a full reset clears the selection colours"
    );
    session.feed(b"\x1bc");
    assert_eq!(session.term.color(ColorKey::SelectionBackground), None);
    assert_eq!(session.term.colors().iter().count(), 0);
}

#[test]
fn overrides_survive_a_soft_reset() {
    let mut session = Session::new(10, 4);
    session.feed(b"\x1b]10;#fff\x07\x1b[!p");
    assert!(session.term.color(ColorKey::Foreground).is_some());
}
