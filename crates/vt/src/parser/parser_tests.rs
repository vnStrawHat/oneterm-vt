use std::cell::RefCell;

use super::*;

const _: () = assert!(size_of::<Parser>() <= 512);

/// Bytes that steer random input toward interesting transitions.
const BIAS: &[u8] = b"\x1b[]P_X^;:?<>0123456789m\x07\x18\x1a\\\xc2\x9b\x9c\xe4\xf0";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Print(char),
    Execute(u8),
    Esc {
        intermediates: Vec<u8>,
        ignore: bool,
        byte: u8,
    },
    Csi {
        groups: Vec<Vec<u16>>,
        intermediates: Vec<u8>,
        ignore: bool,
        byte: u8,
    },
    Osc {
        code: Option<u32>,
        params: Vec<Vec<u8>>,
        term: StringTerm,
        truncated: bool,
    },
    Hook {
        groups: Vec<Vec<u16>>,
        intermediates: Vec<u8>,
        ignore: bool,
        byte: u8,
    },
    Put(u8),
    Unhook(bool),
    ApcStart(u8),
    ApcPut(u8),
    ApcEnd(bool),
}

/// Records every call, expanding `print_str` to one entry per char, and checks the guarantees
/// the trait documents.
#[derive(Default)]
struct Recorder {
    actions: Vec<Action>,
    claims: Vec<u32>,
}

fn groups_of(params: &Params) -> Vec<Vec<u16>> {
    assert!(!params.is_empty());
    params.groups().map(<[u16]>::to_vec).collect()
}

impl Dispatch for Recorder {
    fn print_str(&mut self, text: &str) {
        assert!(!text.is_empty());
        for c in text.chars() {
            assert!(!c.is_control(), "control {c:?} in print_str");
            self.actions.push(Action::Print(c));
        }
    }

    fn execute(&mut self, byte: u8) {
        self.actions.push(Action::Execute(byte));
    }

    fn esc(&mut self, intermediates: &Intermediates, ignore: bool, byte: u8) {
        self.actions.push(Action::Esc {
            intermediates: intermediates.as_slice().to_vec(),
            ignore,
            byte,
        });
    }

    fn csi(&mut self, params: &Params, intermediates: &Intermediates, ignore: bool, byte: u8) {
        assert_eq!(ignore, params.ignored());
        self.actions.push(Action::Csi {
            groups: groups_of(params),
            intermediates: intermediates.as_slice().to_vec(),
            ignore,
            byte,
        });
    }

    fn osc(
        &mut self,
        code: Option<u32>,
        params: &OscParams<'_>,
        term: StringTerm,
        truncated: bool,
    ) {
        assert!(!params.is_empty() && params.len() <= MAX_OSC_PARAMS);
        self.actions.push(Action::Osc {
            code,
            params: params.iter().map(<[u8]>::to_vec).collect(),
            term,
            truncated,
        });
    }

    fn osc_allows_large(&self, code: u32) -> bool {
        self.claims.contains(&code)
    }

    fn dcs_hook(&mut self, params: &Params, intermediates: &Intermediates, ignore: bool, byte: u8) {
        assert_eq!(ignore, params.ignored());
        self.actions.push(Action::Hook {
            groups: groups_of(params),
            intermediates: intermediates.as_slice().to_vec(),
            ignore,
            byte,
        });
    }

    fn dcs_put(&mut self, byte: u8) {
        assert!(byte <= 0x7E && byte != 0x18 && byte != 0x1A && byte != 0x1B);
        self.actions.push(Action::Put(byte));
    }

    fn dcs_unhook(&mut self, aborted: bool) {
        self.actions.push(Action::Unhook(aborted));
    }

    fn apc_start(&mut self, introducer: u8) {
        self.actions.push(Action::ApcStart(introducer));
    }

    fn apc_put(&mut self, byte: u8) {
        self.actions.push(Action::ApcPut(byte));
    }

    fn apc_end(&mut self, aborted: bool) {
        self.actions.push(Action::ApcEnd(aborted));
    }
}

fn feed(bytes: &[u8]) -> Vec<Action> {
    feed_chunked(bytes, bytes.len().max(1))
}

fn feed_chunked(bytes: &[u8], size: usize) -> Vec<Action> {
    let mut parser = Parser::new();
    let mut recorder = Recorder {
        claims: vec![52],
        ..Recorder::default()
    };
    for chunk in bytes.chunks(size.max(1)) {
        parser.advance(&mut recorder, chunk);
    }
    recorder.actions
}

/// Feeds the pieces as separate chunks, in order.
fn feed_split(pieces: &[&[u8]]) -> Vec<Action> {
    let mut parser = Parser::new();
    let mut recorder = Recorder::default();
    for piece in pieces {
        parser.advance(&mut recorder, piece);
    }
    recorder.actions
}

/// Asserts that one-byte chunks give the same log as the whole input; returns the log.
fn feed_both_ways(bytes: &[u8]) -> Vec<Action> {
    let whole = feed(bytes);
    assert_eq!(whole, feed_chunked(bytes, 1), "chunking changed the log");
    whole
}

fn prints(text: &str) -> Vec<Action> {
    text.chars().map(Action::Print).collect()
}

fn esc(intermediates: &[u8], ignore: bool, byte: u8) -> Action {
    Action::Esc {
        intermediates: intermediates.to_vec(),
        ignore,
        byte,
    }
}

fn csi(groups: &[&[u16]], intermediates: &[u8], ignore: bool, byte: u8) -> Action {
    Action::Csi {
        groups: groups.iter().map(|group| group.to_vec()).collect(),
        intermediates: intermediates.to_vec(),
        ignore,
        byte,
    }
}

fn hook(groups: &[&[u16]], intermediates: &[u8], ignore: bool, byte: u8) -> Action {
    Action::Hook {
        groups: groups.iter().map(|group| group.to_vec()).collect(),
        intermediates: intermediates.to_vec(),
        ignore,
        byte,
    }
}

fn osc(code: Option<u32>, params: &[&[u8]], term: StringTerm) -> Action {
    Action::Osc {
        code,
        params: params.iter().map(|param| param.to_vec()).collect(),
        term,
        truncated: false,
    }
}

fn puts(bytes: &[u8]) -> Vec<Action> {
    bytes.iter().map(|&byte| Action::Put(byte)).collect()
}

fn apc_puts(bytes: &[u8]) -> Vec<Action> {
    bytes.iter().map(|&byte| Action::ApcPut(byte)).collect()
}

fn concat(parts: Vec<Vec<Action>>) -> Vec<Action> {
    parts.into_iter().flatten().collect()
}

// ---- states -----------------------------------------------------------------------------------

#[test]
fn ground_prints_and_executes() {
    let expected = vec![
        Action::Print('a'),
        Action::Print('b'),
        Action::Execute(0x07),
        Action::Execute(0x7F),
    ];
    assert_eq!(feed_both_ways(b"ab\x07\x7f"), expected);
}

#[test]
fn escape_executes_c0_and_stays() {
    let expected = vec![Action::Execute(0x07), esc(b"", false, b'A')];
    assert_eq!(feed_both_ways(b"\x1b\x07A"), expected);
}

#[test]
fn escape_is_idempotent() {
    let expected = vec![csi(&[&[0]], b"", false, b'A')];
    assert_eq!(feed_both_ways(b"\x1b\x1b\x1b[A"), expected);
}

#[test]
fn can_sub_cancel_every_header_state() {
    let prefixes: [&[u8]; 10] = [
        b"\x1b", b"\x1b(", b"\x1b[", b"\x1b[1", b"\x1b[ ", b"\x1b[1?", b"\x1bP", b"\x1bP1",
        b"\x1bP ", b"\x1bP1?",
    ];
    for prefix in prefixes {
        for cancel in [0x18u8, 0x1A] {
            let mut input = prefix.to_vec();
            input.extend([cancel, b'A']);
            let expected = vec![Action::Execute(cancel), Action::Print('A')];
            assert_eq!(feed_both_ways(&input), expected, "{prefix:?} {cancel:#x}");
        }
    }
}

#[test]
fn escape_intermediates() {
    assert_eq!(feed_both_ways(b"\x1b#8"), vec![esc(b"#", false, b'8')]);
    assert_eq!(feed_both_ways(b"\x1b(\x7fB"), vec![esc(b"(", false, b'B')]);
    assert_eq!(feed_both_ways(b"\x1b P"), vec![esc(b" ", false, b'P')]);
}

#[test]
fn escape_third_intermediate_sets_ignore() {
    assert_eq!(feed_both_ways(b"\x1b !#8"), vec![esc(b" !", true, b'8')]);
}

#[test]
fn csi_entry_forms() {
    assert_eq!(
        feed_both_ways(b"\x1b[m"),
        vec![csi(&[&[0]], b"", false, b'm')]
    );
    assert_eq!(
        feed_both_ways(b"\x1b[ q"),
        vec![csi(&[&[0]], b" ", false, b'q')]
    );
    assert_eq!(
        feed_both_ways(b"\x1b[?25h"),
        vec![csi(&[&[25]], b"?", false, b'h')]
    );
    assert_eq!(
        feed_both_ways(b"\x1b[1\x07;2m"),
        vec![Action::Execute(7), csi(&[&[1], &[2]], b"", false, b'm')]
    );
}

#[test]
fn csi_param_after_intermediate_dispatches_ignored() {
    let log = feed_both_ways(b"\x1b[ 1m");
    assert!(matches!(
        log.as_slice(),
        [Action::Csi {
            ignore: true,
            byte: b'm',
            ..
        }]
    ));
}

#[test]
fn csi_marker_after_param_dispatches_ignored() {
    assert_eq!(
        feed_both_ways(b"\x1b[1?m"),
        vec![csi(&[&[1]], b"", true, b'm')]
    );
    let log = feed_both_ways(b"\x1b[?1;2?\x07m");
    assert_eq!(log.first(), Some(&Action::Execute(7)));
    assert!(matches!(
        log.last(),
        Some(Action::Csi {
            ignore: true,
            byte: b'm',
            ..
        })
    ));
    assert_eq!(log.len(), 2);
}

#[test]
fn csi_drops_high_bytes() {
    assert_eq!(
        feed_both_ways(b"\x1b[1\x9bm"),
        vec![csi(&[&[1]], b"", false, b'm')]
    );
}

#[test]
fn dcs_header_drops_c0() {
    let expected = vec![
        hook(&[&[0]], b"", false, b'q'),
        Action::Unhook(false),
        esc(b"", false, b'\\'),
    ];
    assert_eq!(feed_both_ways(b"\x1bP\x07q\x1b\\"), expected);
}

#[test]
fn dcs_streams_payload() {
    let expected = concat(vec![
        vec![hook(&[&[1], &[2]], b"", false, b'q')],
        puts(b"#0~\x07"),
        vec![Action::Unhook(false), esc(b"", false, b'\\')],
    ]);
    assert_eq!(feed_both_ways(b"\x1bP1;2q#0~\x07\x1b\\"), expected);
}

#[test]
fn dcs_drops_high_bytes_and_c1_st() {
    let expected = concat(vec![
        vec![hook(&[&[0]], b"", false, b'q')],
        puts(b"ab"),
        vec![Action::Unhook(false), esc(b"", false, b'\\')],
    ]);
    assert_eq!(feed_both_ways(b"\x1bPqa\x9cb\x7f\x1b\\"), expected);
}

#[test]
fn dcs_malformed_header_hooks_ignored() {
    let expected = concat(vec![
        vec![hook(&[&[1]], b"", true, b'q')],
        puts(b"xy"),
        vec![Action::Unhook(false), esc(b"", false, b'\\')],
    ]);
    assert_eq!(feed_both_ways(b"\x1bP1?qxy\x1b\\"), expected);
}

#[test]
fn dcs_can_aborts() {
    let expected = concat(vec![
        vec![hook(&[&[0]], b"", false, b'q')],
        puts(b"ab"),
        vec![Action::Unhook(true), Action::Execute(0x18)],
    ]);
    assert_eq!(feed_both_ways(b"\x1bPqab\x18"), expected);
}

#[test]
fn apc_sos_pm_share_a_state() {
    let apc = concat(vec![
        vec![Action::ApcStart(b'_')],
        apc_puts(b"Gi=1"),
        vec![Action::ApcEnd(false), esc(b"", false, b'\\')],
    ]);
    assert_eq!(feed_both_ways(b"\x1b_Gi=1\x1b\\"), apc);
    let sos = vec![
        Action::ApcStart(b'X'),
        Action::ApcPut(b'!'),
        Action::ApcEnd(true),
        Action::Execute(0x18),
    ];
    assert_eq!(feed_both_ways(b"\x1bX!\x18"), sos);
    let pm = concat(vec![
        vec![Action::ApcStart(b'^')],
        apc_puts(b"x\x07\x9c"),
        vec![Action::ApcEnd(false), esc(b"", false, b'\\')],
    ]);
    assert_eq!(feed_both_ways(b"\x1b^x\x07\x9c\x1b\\"), pm);
}

#[test]
fn osc_bel_and_st() {
    assert_eq!(
        feed_both_ways(b"\x1b]0;hi\x07"),
        vec![osc(Some(0), &[b"0", b"hi"], StringTerm::Bel)]
    );
    assert_eq!(
        feed_both_ways(b"\x1b]2;t\x1b\\"),
        vec![
            osc(Some(2), &[b"2", b"t"], StringTerm::St),
            esc(b"", false, b'\\')
        ]
    );
}

#[test]
fn osc_keeps_utf8_and_c1_payload() {
    assert_eq!(
        feed_both_ways(b"\x1b]2;\xc3\xa9\x9c\x07"),
        vec![osc(Some(2), &[b"2", b"\xc3\xa9\x9c"], StringTerm::Bel)]
    );
}

#[test]
fn osc_drops_c0() {
    assert_eq!(
        feed_both_ways(b"\x1b]0;a\x01b\x07"),
        vec![osc(Some(0), &[b"0", b"ab"], StringTerm::Bel)]
    );
}

#[test]
fn osc_cancelled_by_can_is_not_dispatched() {
    assert_eq!(feed_both_ways(b"\x1b]0;x\x18"), vec![Action::Execute(0x18)]);
    assert_eq!(feed_both_ways(b"\x1b]0;x\x1a"), vec![Action::Execute(0x1A)]);
}

#[test]
fn osc_code_parsing() {
    type Case<'a> = (&'a [u8], Option<u32>, &'a [&'a [u8]]);
    let cases: [Case<'_>; 5] = [
        (b"\x1b]\x07", None, &[b""]),
        (b"\x1b]52;c;Zm9v\x07", Some(52), &[b"52", b"c", b"Zm9v"]),
        (b"\x1b]x1;y\x07", None, &[b"x1", b"y"]),
        (b"\x1b]4294967295\x07", Some(u32::MAX), &[b"4294967295"]),
        (b"\x1b]4294967296;a\x07", None, &[b"4294967296", b"a"]),
    ];
    for (input, code, params) in cases {
        assert_eq!(
            feed_both_ways(input),
            vec![osc(code, params, StringTerm::Bel)],
            "{input:?}"
        );
    }
}

#[test]
fn osc_params_past_limit_join_into_last() {
    let mut input = b"\x1b]".to_vec();
    let numbers: Vec<String> = (0..20).map(|n| n.to_string()).collect();
    input.extend(numbers.join(";").as_bytes());
    input.push(7);
    let log = feed_both_ways(&input);
    let [Action::Osc { params, .. }] = log.as_slice() else {
        panic!("expected one Osc, got {log:?}");
    };
    assert_eq!(params.len(), 16);
    assert_eq!(params[15], b"15;16;17;18;19");
}

#[derive(Default)]
struct ClaimCounter {
    asked: RefCell<Vec<u32>>,
}

impl Dispatch for ClaimCounter {
    fn print_str(&mut self, _text: &str) {}
    fn execute(&mut self, _byte: u8) {}
    fn osc_allows_large(&self, code: u32) -> bool {
        self.asked.borrow_mut().push(code);
        false
    }
}

#[test]
fn osc_allows_large_asked_once_per_osc() {
    let asked_for = |input: &[u8]| {
        let mut sink = ClaimCounter::default();
        Parser::new().advance(&mut sink, input);
        sink.asked.into_inner()
    };
    assert_eq!(asked_for(b"\x1b]52;c;x\x07"), vec![52]);
    assert_eq!(asked_for(b"\x1b]52;c;x;y;z\x07"), vec![52]);
    assert_eq!(asked_for(b"\x1b]52\x07"), vec![52]);
    assert!(asked_for(b"\x1b]\x07").is_empty());
    assert!(asked_for(b"\x1b]x;y\x07").is_empty());
}

// ---- params, intermediates, UTF-8 ---------------------------------------------------------------

#[derive(Default)]
struct ParamsCapture(Option<Params>);

impl Dispatch for ParamsCapture {
    fn print_str(&mut self, _text: &str) {}
    fn execute(&mut self, _byte: u8) {}
    fn csi(&mut self, params: &Params, _: &Intermediates, _: bool, _: u8) {
        self.0 = Some(params.clone());
    }
}

fn params_of(input: &[u8]) -> Params {
    let mut sink = ParamsCapture::default();
    Parser::new().advance(&mut sink, input);
    sink.0.expect("no csi dispatched")
}

#[test]
fn params_table() {
    let ones: String = vec!["1"; 33].join(";");
    let cases: Vec<(String, Vec<Vec<u16>>, bool)> = vec![
        ("m".into(), vec![vec![0]], false),
        (";4m".into(), vec![vec![0], vec![4]], false),
        ("4;m".into(), vec![vec![4], vec![0]], false),
        (":1m".into(), vec![vec![0, 1]], false),
        ("1:2;3m".into(), vec![vec![1, 2], vec![3]], false),
        (
            "38:2::255:0:0m".into(),
            vec![vec![38, 2, 0, 255, 0, 0]],
            false,
        ),
        (
            "38;2;255;0;0m".into(),
            vec![vec![38], vec![2], vec![255], vec![0], vec![0]],
            false,
        ),
        ("?25h".into(), vec![vec![25]], false),
        (format!("{ones}m"), vec![vec![1]; 32], true),
        ("1?m".into(), vec![vec![1]], true),
    ];
    for (tail, groups, ignored) in cases {
        let params = params_of(format!("\x1b[{tail}").as_bytes());
        let actual: Vec<Vec<u16>> = params.groups().map(<[u16]>::to_vec).collect();
        assert_eq!(actual, groups, "{tail}");
        assert_eq!(params.ignored(), ignored, "{tail}");
    }
}

#[test]
fn params_seps_kept() {
    use ParamSep::{Colon as C, Semicolon as S};
    let params = params_of(b"\x1b[38:2::1:2:3;4m");
    let seps: Vec<ParamSep> = (0..params.len()).map(|i| params.sep(i).unwrap()).collect();
    assert_eq!(seps, [S, C, C, C, C, C, S]);
    assert_eq!(params.get(6), Some(4));
    assert_eq!(params.get(7), None);
    assert_eq!(params.sep(7), None);
}

#[test]
fn params_overflow() {
    let list = |count: usize| vec!["1"; count].join(";");
    let full = params_of(format!("\x1b[{}m", list(32)).as_bytes());
    assert_eq!((full.len(), full.ignored()), (32, false));
    let over = params_of(format!("\x1b[{}m", list(33)).as_bytes());
    assert_eq!((over.len(), over.ignored()), (32, true));
    let trailing = params_of(format!("\x1b[{};m", list(32)).as_bytes());
    assert_eq!((trailing.len(), trailing.ignored()), (32, true));
    assert!(over.values().iter().all(|&value| value == 1));
}

#[test]
fn param_saturates_at_u16_max() {
    let expected = vec![csi(&[&[65535]], b"", false, b'm')];
    assert_eq!(feed_both_ways(b"\x1b[99999999999m"), expected);
}

#[test]
fn c1_is_executed_never_an_introducer() {
    let expected = vec![
        Action::Execute(0x80),
        Action::Execute(0x90),
        Action::Execute(0x9B),
        Action::Execute(0x9C),
        Action::Execute(0x9D),
        Action::Execute(0x9F),
        Action::Print('a'),
    ];
    assert_eq!(feed_both_ways(b"\x80\x90\x9b\x9c\x9d\x9fa"), expected);
    let encoded = vec![
        Action::Execute(0x9B),
        Action::Print('['),
        Action::Print('A'),
    ];
    assert_eq!(feed_both_ways(b"\xc2\x9b[A"), encoded);
}

#[test]
fn utf8_cases() {
    assert_eq!(
        feed_both_ways(b"a\xef\xbcb"),
        vec![
            Action::Print('a'),
            Action::Print('\u{FFFD}'),
            Action::Print('b')
        ]
    );
    assert_eq!(
        feed_both_ways(b"\xe0\x80"),
        vec![Action::Print('\u{FFFD}'), Action::Execute(0x80)]
    );
    assert_eq!(
        feed_both_ways("\u{1F389}_".as_bytes()),
        vec![Action::Print('\u{1F389}'), Action::Print('_')]
    );
    assert_eq!(
        feed_both_ways(b"\xd8\x1b012"),
        concat(vec![
            vec![Action::Print('\u{FFFD}'), esc(b"", false, b'0')],
            prints("12")
        ])
    );
}

#[test]
fn utf8_carry_cases() {
    let cases: Vec<(Vec<&[u8]>, Vec<Action>)> = vec![
        (vec![b"\xf0\x9f", b"\x9a\x80"], prints("\u{1F680}")),
        (
            vec![b"\xc4", b"\xb8\xf0\x9f\x8e\x89"],
            prints("\u{138}\u{1F389}"),
        ),
        (vec![b"\xe4\xbf", b"\x99\xb5"], prints("\u{4FD9}\u{FFFD}")),
        (
            vec![b"\xc0\xc5", b"\x93@\x97"],
            concat(vec![
                prints("\u{FFFD}\u{153}@"),
                vec![Action::Execute(0x97)],
            ]),
        ),
        (
            vec![b"\xe4", b"\x1b7"],
            vec![Action::Print('\u{FFFD}'), esc(b"", false, b'7')],
        ),
        (
            vec![b"\xf0", b"\x9f", b"\x9a", b"\x80"],
            prints("\u{1F680}"),
        ),
        (vec![b"\xf0\x9f", b"\x9a", b"A"], prints("\u{FFFD}A")),
        (vec![b"\xc2", b"\x9b"], vec![Action::Execute(0x9B)]),
    ];
    for (pieces, expected) in cases {
        let joined: Vec<u8> = pieces.concat();
        assert_eq!(feed(&joined), expected, "whole {joined:?}");
        assert_eq!(feed_split(&pieces), expected, "split {pieces:?}");
    }
}

#[derive(Default)]
struct RunCollector(Vec<String>);

impl Dispatch for RunCollector {
    fn print_str(&mut self, text: &str) {
        self.0.push(text.to_owned());
    }
    fn execute(&mut self, _byte: u8) {}
}

fn runs_of(input: &[u8]) -> Vec<String> {
    let mut sink = RunCollector::default();
    Parser::new().advance(&mut sink, input);
    sink.0
}

#[test]
fn print_runs_are_batched() {
    assert_eq!(runs_of(&[b'x'; 4096]), vec!["x".repeat(4096)]);
    assert_eq!(runs_of(b"ab\ncd\r\nef"), ["ab", "cd", "ef"]);
    assert_eq!(runs_of(b"ab\tcd"), ["ab", "cd"]);
    assert_eq!(runs_of("a\u{85}b".as_bytes()), ["a", "b"]);
}

#[derive(Default)]
struct Counts {
    prints: usize,
    executes: usize,
}

impl Dispatch for Counts {
    fn print_str(&mut self, _text: &str) {
        self.prints += 1;
    }
    fn execute(&mut self, _byte: u8) {
        self.executes += 1;
    }
}

#[test]
fn invalid_run_stays_linear() {
    let mut counts = Counts::default();
    Parser::new().advance(&mut counts, &vec![0xFF; 1 << 20]);
    assert_eq!(counts.prints, 1 << 20);
    assert_eq!(counts.executes, 0);
}

#[test]
fn reset_returns_to_ground() {
    let mut parser = Parser::new();
    let mut recorder = Recorder::default();
    parser.advance(&mut recorder, b"\x1b]0;partial");
    parser.reset();
    parser.advance(&mut recorder, b"hi");
    assert_eq!(recorder.actions, prints("hi"));

    let mut recorder = Recorder::default();
    parser.advance(&mut recorder, b"\xe4");
    parser.reset();
    parser.advance(&mut recorder, b"a");
    assert_eq!(recorder.actions, prints("a"));

    let mut recorder = Recorder::default();
    parser.advance(&mut recorder, b"\x1bPqab");
    parser.reset();
    parser.advance(&mut recorder, b"c");
    assert_eq!(
        recorder.actions,
        concat(vec![
            vec![hook(&[&[0]], b"", false, b'q')],
            puts(b"ab"),
            prints("c")
        ])
    );
}

// ---- hostile input ---------------------------------------------------------------------------

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn million_random_bytes_never_panic() {
    let mut state = 0x5EED_0001_u64;
    let mut bytes = Vec::with_capacity(1 << 20);
    for index in 0..(1usize << 20) {
        let random = xorshift(&mut state);
        let byte = if index % 3 == 2 {
            BIAS[(random >> 8) as usize % BIAS.len()]
        } else {
            random as u8
        };
        bytes.push(byte);
    }
    let mut parser = Parser::new();
    let mut counts = Counts::default();
    for chunk in bytes.chunks(8 * 1024) {
        parser.advance(&mut counts, chunk);
    }
    assert!(counts.prints + counts.executes > 0);

    let sample = &bytes[..64 * 1024];
    let whole = feed(sample);
    for size in [1, 2, 3, 7, 997, 4096] {
        assert_eq!(feed_chunked(sample, size), whole, "chunk size {size}");
    }
}

mod props {
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::*;

    proptest! {
        #[test]
        fn chunking_is_invariant(
            bytes in prop::collection::vec(
                prop_oneof![any::<u8>(), select(BIAS.to_vec())],
                0..2048,
            ),
            sizes in prop::collection::vec(1usize..=64, 1..32),
        ) {
            let whole = feed(&bytes);
            let mut parser = Parser::new();
            let mut recorder = Recorder { claims: vec![52], ..Recorder::default() };
            let mut rest = bytes.as_slice();
            for size in sizes.iter().cycle() {
                if rest.is_empty() {
                    break;
                }
                let (chunk, tail) = rest.split_at(rest.len().min(*size));
                parser.advance(&mut recorder, chunk);
                rest = tail;
            }
            prop_assert_eq!(recorder.actions, whole);
        }
    }
}
