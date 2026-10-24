//! Tests of the legacy rung: the control table, every named key and the worked rows.

use super::*;

const NONE: KeyMods = KeyMods {
    shift: false,
    ctrl: false,
    alt: false,
};
const SHIFT: KeyMods = KeyMods {
    shift: true,
    ctrl: false,
    alt: false,
};
const CTRL: KeyMods = KeyMods {
    shift: false,
    ctrl: true,
    alt: false,
};
const ALT: KeyMods = KeyMods {
    shift: false,
    ctrl: false,
    alt: true,
};
const CTRL_ALT: KeyMods = KeyMods {
    shift: false,
    ctrl: true,
    alt: true,
};
const SHIFT_ALT: KeyMods = KeyMods {
    shift: true,
    ctrl: false,
    alt: true,
};

fn text(text: &str) -> KeySpec {
    KeySpec::Character(text.to_owned())
}

fn named(key: NamedKey) -> KeySpec {
    KeySpec::Named(key)
}

/// Shadows the public function: a default snapshot, with DECCKM as `app_cursor`.
fn encode_key(key: &KeySpec, mods: KeyMods, app_cursor: bool) -> Option<Vec<u8>> {
    let modes = ModeSnapshot {
        app_cursor,
        ..ModeSnapshot::default()
    };
    super::encode_key(key, mods, &modes)
}

fn bytes(text: &str) -> Option<Vec<u8>> {
    Some(text.as_bytes().to_vec())
}

/// Expected bytes of one named key: plain, plain with DECCKM, then Shift, Ctrl, Alt, Ctrl+Alt.
type NamedRow = (NamedKey, [&'static str; 6]);

const CONDITIONS: [(KeyMods, bool); 6] = [
    (NONE, false),
    (NONE, true),
    (SHIFT, false),
    (CTRL, false),
    (ALT, false),
    (CTRL_ALT, false),
];

/// `CSI n ~`, or `CSI n ; m ~`.
macro_rules! tilde {
    ($key:ident, $n:literal) => {
        (
            NamedKey::$key,
            [
                concat!("\x1b[", $n, "~"),
                concat!("\x1b[", $n, "~"),
                concat!("\x1b[", $n, ";2~"),
                concat!("\x1b[", $n, ";5~"),
                concat!("\x1b[", $n, ";3~"),
                concat!("\x1b[", $n, ";7~"),
            ],
        )
    };
}

/// Cursor keys: `CSI X`, `SS3 X` under DECCKM, `CSI 1 ; m X`.
macro_rules! cursor {
    ($key:ident, $letter:literal) => {
        (
            NamedKey::$key,
            [
                concat!("\x1b[", $letter),
                concat!("\x1bO", $letter),
                concat!("\x1b[1;2", $letter),
                concat!("\x1b[1;5", $letter),
                concat!("\x1b[1;3", $letter),
                concat!("\x1b[1;7", $letter),
            ],
        )
    };
}

/// F1, F2, F4: `SS3 X`, `CSI 1 ; m X`.
macro_rules! ss3 {
    ($key:ident, $letter:literal) => {
        (
            NamedKey::$key,
            [
                concat!("\x1bO", $letter),
                concat!("\x1bO", $letter),
                concat!("\x1b[1;2", $letter),
                concat!("\x1b[1;5", $letter),
                concat!("\x1b[1;3", $letter),
                concat!("\x1b[1;7", $letter),
            ],
        )
    };
}

/// F13-F24: the same bytes whatever is held.
macro_rules! fixed {
    ($key:ident, $bytes:literal) => {
        (NamedKey::$key, [$bytes; 6])
    };
}

const NAMED_TABLE: [NamedRow; 38] = [
    (
        NamedKey::Enter,
        [
            "\r",
            "\r",
            "\x1b[13;2u",
            "\x1b[13;5u",
            "\x1b\r",
            "\x1b[13;7u",
        ],
    ),
    (
        NamedKey::Backspace,
        ["\x7f", "\x7f", "\x7f", "\x08", "\x1b\x7f", "\x1b\x08"],
    ),
    tilde!(Delete, "3"),
    (
        NamedKey::Tab,
        ["\t", "\t", "\x1b[Z", "\t", "\x1b\t", "\x1b\t"],
    ),
    (
        NamedKey::Escape,
        ["\x1b", "\x1b", "\x1b", "\x1b", "\x1b\x1b", "\x1b\x1b"],
    ),
    cursor!(ArrowUp, "A"),
    cursor!(ArrowDown, "B"),
    cursor!(ArrowLeft, "D"),
    cursor!(ArrowRight, "C"),
    cursor!(Home, "H"),
    cursor!(End, "F"),
    tilde!(PageUp, "5"),
    tilde!(PageDown, "6"),
    tilde!(Insert, "2"),
    ss3!(F1, "P"),
    ss3!(F2, "Q"),
    (
        NamedKey::F3,
        [
            "\x1bOR",
            "\x1bOR",
            "\x1b[13;2~",
            "\x1b[13;5~",
            "\x1b[13;3~",
            "\x1b[13;7~",
        ],
    ),
    ss3!(F4, "S"),
    tilde!(F5, "15"),
    tilde!(F6, "17"),
    tilde!(F7, "18"),
    tilde!(F8, "19"),
    tilde!(F9, "20"),
    tilde!(F10, "21"),
    tilde!(F11, "23"),
    tilde!(F12, "24"),
    fixed!(F13, "\x1b[1;2P"),
    fixed!(F14, "\x1b[1;2Q"),
    fixed!(F15, "\x1b[28~"),
    fixed!(F16, "\x1b[1;2S"),
    fixed!(F17, "\x1b[15;2~"),
    fixed!(F18, "\x1b[17;2~"),
    fixed!(F19, "\x1b[18;2~"),
    fixed!(F20, "\x1b[19;2~"),
    fixed!(F21, "\x1b[20;2~"),
    fixed!(F22, "\x1b[21;2~"),
    fixed!(F23, "\x1b[23;2~"),
    fixed!(F24, "\x1b[24;2~"),
];

/// Ctrl plus one ASCII scalar (legacy.md control table).
const CONTROL_TABLE: [(&str, Option<u8>); 52] = [
    ("a", Some(0x01)),
    ("c", Some(0x03)),
    ("m", Some(0x0d)),
    ("z", Some(0x1a)),
    ("A", Some(0x01)),
    ("C", Some(0x03)),
    ("Z", Some(0x1a)),
    (" ", Some(0x00)),
    ("2", Some(0x00)),
    ("@", Some(0x00)),
    ("3", Some(0x1b)),
    ("[", Some(0x1b)),
    ("4", Some(0x1c)),
    ("\\", Some(0x1c)),
    ("5", Some(0x1d)),
    ("]", Some(0x1d)),
    ("6", Some(0x1e)),
    ("^", Some(0x1e)),
    ("~", Some(0x1e)),
    ("7", Some(0x1f)),
    ("_", Some(0x1f)),
    ("/", Some(0x1f)),
    ("8", Some(0x7f)),
    ("?", Some(0x7f)),
    ("0", Some(b'0')),
    ("1", Some(b'1')),
    ("9", Some(b'9')),
    (",", Some(b',')),
    (".", Some(b'.')),
    (";", Some(b';')),
    ("'", Some(b'\'')),
    ("-", Some(b'-')),
    ("=", Some(b'=')),
    ("`", Some(b'`')),
    ("!", Some(b'!')),
    ("#", Some(b'#')),
    ("$", Some(b'$')),
    ("%", Some(b'%')),
    ("&", Some(b'&')),
    ("*", Some(b'*')),
    ("(", Some(b'(')),
    (")", Some(b')')),
    ("+", Some(b'+')),
    ("{", Some(b'{')),
    ("}", Some(b'}')),
    ("|", Some(b'|')),
    (":", Some(b':')),
    ("\"", Some(b'"')),
    ("<", Some(b'<')),
    (">", Some(b'>')),
    ("é", None),
    ("ab", None),
];

/// The worked rows of legacy.md: key, modifiers, DECCKM, bytes.
fn worked_rows() -> Vec<(KeySpec, KeyMods, bool, Option<Vec<u8>>)> {
    let shift_ctrl = KeyMods {
        shift: true,
        ctrl: true,
        alt: false,
    };
    vec![
        (text("c"), CTRL, false, bytes("\x03")),
        (text(" "), CTRL, false, bytes("\x00")),
        (text("A"), shift_ctrl, false, bytes("\x01")),
        (text("0"), CTRL, false, bytes("0")),
        (text("9"), CTRL, false, bytes("9")),
        (text(","), CTRL, false, bytes(",")),
        (text("."), CTRL, false, bytes(".")),
        (text("é"), CTRL, false, None),
        (text("ab"), CTRL, false, None),
        (text("a"), ALT, false, bytes("\x1ba")),
        (text("a"), CTRL_ALT, false, bytes("\x1b\x01")),
        (text("["), CTRL_ALT, false, bytes("\x1b\x1b")),
        (text("a"), NONE, false, bytes("a")),
        (text("A"), SHIFT, false, bytes("A")),
        (text("é"), NONE, false, bytes("é")),
        (text("ab"), NONE, false, bytes("ab")),
        (text("é"), ALT, false, bytes("\x1bé")),
        (named(NamedKey::Enter), NONE, false, bytes("\r")),
        (named(NamedKey::Enter), SHIFT, false, bytes("\x1b[13;2u")),
        (named(NamedKey::Enter), ALT, false, bytes("\x1b\r")),
        (named(NamedKey::Backspace), NONE, false, bytes("\x7f")),
        (named(NamedKey::Backspace), CTRL, false, bytes("\x08")),
        (named(NamedKey::Backspace), ALT, false, bytes("\x1b\x7f")),
        (
            named(NamedKey::Backspace),
            CTRL_ALT,
            false,
            bytes("\x1b\x08"),
        ),
        (named(NamedKey::Delete), CTRL, false, bytes("\x1b[3;5~")),
        (named(NamedKey::Tab), SHIFT, false, bytes("\x1b[Z")),
        (named(NamedKey::Tab), ALT, false, bytes("\x1b\t")),
        (named(NamedKey::Tab), SHIFT_ALT, false, bytes("\x1b\x1b[Z")),
        (named(NamedKey::Escape), ALT, false, bytes("\x1b\x1b")),
        (named(NamedKey::ArrowUp), NONE, false, bytes("\x1b[A")),
        (named(NamedKey::ArrowUp), NONE, true, bytes("\x1bOA")),
        (named(NamedKey::ArrowUp), CTRL, true, bytes("\x1b[1;5A")),
        (named(NamedKey::ArrowUp), ALT, true, bytes("\x1b[1;3A")),
        (named(NamedKey::Home), NONE, true, bytes("\x1bOH")),
        (named(NamedKey::End), NONE, true, bytes("\x1bOF")),
        (named(NamedKey::Home), CTRL, true, bytes("\x1b[1;5H")),
        (named(NamedKey::Home), ALT, true, bytes("\x1b[1;3H")),
        (named(NamedKey::PageUp), CTRL, false, bytes("\x1b[5;5~")),
        (named(NamedKey::PageUp), ALT, false, bytes("\x1b[5;3~")),
        (named(NamedKey::F1), NONE, false, bytes("\x1bOP")),
        (named(NamedKey::F5), NONE, false, bytes("\x1b[15~")),
        (named(NamedKey::F12), NONE, false, bytes("\x1b[24~")),
        (named(NamedKey::F1), CTRL, false, bytes("\x1b[1;5P")),
        (named(NamedKey::F3), SHIFT, false, bytes("\x1b[13;2~")),
        (named(NamedKey::F5), CTRL, false, bytes("\x1b[15;5~")),
        (named(NamedKey::Insert), ALT, false, bytes("\x1b[2;3~")),
        (named(NamedKey::F13), NONE, false, bytes("\x1b[1;2P")),
        (named(NamedKey::F15), CTRL_ALT, false, bytes("\x1b[28~")),
        (named(NamedKey::F17), NONE, false, bytes("\x1b[15;2~")),
        (named(NamedKey::F24), CTRL, false, bytes("\x1b[24;2~")),
    ]
}

#[test]
fn legacy_table() {
    for (scalar, want) in CONTROL_TABLE {
        let got = encode_key(&text(scalar), CTRL, false);
        assert_eq!(got, want.map(|byte| vec![byte]), "Ctrl+{scalar:?}");
    }
    for (key, want) in NAMED_TABLE {
        for ((mods, app_cursor), want) in CONDITIONS.into_iter().zip(want) {
            let got = encode_key(&named(key), mods, app_cursor);
            assert_eq!(
                got,
                bytes(want),
                "{key:?} mods {mods:?} app_cursor {app_cursor}"
            );
        }
    }
    for (index, (key, mods, app_cursor, want)) in worked_rows().into_iter().enumerate() {
        let got = encode_key(&key, mods, app_cursor);
        assert_eq!(
            got, want,
            "worked row {index}: {key:?} {mods:?} {app_cursor}"
        );
    }
}

/// The index of a key in `ALL_KEYS`; the `match` has no wildcard, so a new key breaks the build.
fn position(key: NamedKey) -> usize {
    match key {
        NamedKey::Enter => 0,
        NamedKey::Backspace => 1,
        NamedKey::Delete => 2,
        NamedKey::Tab => 3,
        NamedKey::Escape => 4,
        NamedKey::ArrowUp => 5,
        NamedKey::ArrowDown => 6,
        NamedKey::ArrowLeft => 7,
        NamedKey::ArrowRight => 8,
        NamedKey::Home => 9,
        NamedKey::End => 10,
        NamedKey::PageUp => 11,
        NamedKey::PageDown => 12,
        NamedKey::Insert => 13,
        NamedKey::F1 => 14,
        NamedKey::F2 => 15,
        NamedKey::F3 => 16,
        NamedKey::F4 => 17,
        NamedKey::F5 => 18,
        NamedKey::F6 => 19,
        NamedKey::F7 => 20,
        NamedKey::F8 => 21,
        NamedKey::F9 => 22,
        NamedKey::F10 => 23,
        NamedKey::F11 => 24,
        NamedKey::F12 => 25,
        NamedKey::F13 => 26,
        NamedKey::F14 => 27,
        NamedKey::F15 => 28,
        NamedKey::F16 => 29,
        NamedKey::F17 => 30,
        NamedKey::F18 => 31,
        NamedKey::F19 => 32,
        NamedKey::F20 => 33,
        NamedKey::F21 => 34,
        NamedKey::F22 => 35,
        NamedKey::F23 => 36,
        NamedKey::F24 => 37,
    }
}

#[test]
fn every_named_key_is_known_here() {
    let keys: Vec<NamedKey> = NAMED_TABLE.iter().map(|(key, _)| *key).collect();
    assert_eq!(keys.len(), 38);
    for (expected, key) in keys.iter().enumerate() {
        assert_eq!(position(*key), expected, "{key:?}");
    }
    let mut seen: Vec<usize> = keys.iter().map(|key| position(*key)).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 38, "the table names every key once");
    for key in keys {
        for (mods, app_cursor) in CONDITIONS {
            let got = encode_key(&named(key), mods, app_cursor);
            assert!(got.is_some_and(|b| !b.is_empty()), "{key:?} encodes");
        }
    }
}

#[test]
fn only_app_cursor_is_read() {
    let flips: [fn(&mut ModeSnapshot); 7] = [
        |m| m.alt_screen = true,
        |m| m.app_keypad = true,
        |m| m.bracketed_paste = true,
        |m| m.insert = true,
        |m| m.show_cursor = false,
        |m| m.alternate_scroll = false,
        |m| m.reverse_video = true,
    ];
    let mouse = crate::MouseProtocol {
        reporting: crate::MouseReporting::AnyEvent,
        encoding: crate::MouseEncoding::Sgr,
    };
    for (key, _) in NAMED_TABLE {
        for (mods, app_cursor) in CONDITIONS {
            let base = ModeSnapshot {
                app_cursor,
                ..ModeSnapshot::default()
            };
            let want = super::encode_key(&named(key), mods, &base);
            for flip in flips {
                let mut modes = base;
                flip(&mut modes);
                assert_eq!(
                    super::encode_key(&named(key), mods, &modes),
                    want,
                    "{key:?}"
                );
            }
            let mut modes = base;
            modes.mouse = Some(mouse);
            assert_eq!(
                super::encode_key(&named(key), mods, &modes),
                want,
                "{key:?} mouse"
            );
        }
    }
}

#[test]
fn ctrl_c_is_0x03() {
    assert_eq!(encode_key(&text("c"), CTRL, false), bytes("\x03"));
}

#[test]
fn ctrl_space_is_nul() {
    assert_eq!(encode_key(&text(" "), CTRL, false), bytes("\x00"));
}

#[test]
fn ctrl_uppercase_letter_uses_same_control_byte() {
    for letter in 'a'..='z' {
        let lower = encode_key(&text(&letter.to_string()), CTRL, false);
        let upper = encode_key(&text(&letter.to_ascii_uppercase().to_string()), CTRL, false);
        assert_eq!(lower, upper, "Ctrl+{letter}");
        assert_eq!(lower, Some(vec![letter as u8 - b'a' + 1]));
    }
}

#[test]
fn ctrl_punctuation_and_digits_follow_xterm_table() {
    let rows = [
        ("2", 0x00),
        ("@", 0x00),
        ("3", 0x1b),
        ("[", 0x1b),
        ("4", 0x1c),
        ("\\", 0x1c),
        ("5", 0x1d),
        ("]", 0x1d),
        ("6", 0x1e),
        ("^", 0x1e),
        ("~", 0x1e),
        ("7", 0x1f),
        ("_", 0x1f),
        ("/", 0x1f),
        ("8", 0x7f),
        ("?", 0x7f),
    ];
    for (scalar, want) in rows {
        assert_eq!(
            encode_key(&text(scalar), CTRL, false),
            Some(vec![want]),
            "{scalar}"
        );
    }
}

#[test]
fn ctrl_unmapped_ascii_passes_through() {
    for scalar in ["0", "1", "9", ",", ".", ";"] {
        assert_eq!(
            encode_key(&text(scalar), CTRL, false),
            bytes(scalar),
            "{scalar}"
        );
    }
}

#[test]
fn ctrl_non_ascii_has_no_encoding() {
    assert_eq!(encode_key(&text("é"), CTRL, false), None);
    assert_eq!(encode_key(&text("ab"), CTRL, false), None);
    assert_eq!(encode_key(&text("é"), CTRL_ALT, false), None);
}

#[test]
fn ctrl_alt_letter_prefixes_esc_to_control_byte() {
    assert_eq!(encode_key(&text("a"), CTRL_ALT, false), bytes("\x1b\x01"));
}

#[test]
fn ctrl_alt_bracket_prefixes_esc_to_esc() {
    assert_eq!(encode_key(&text("["), CTRL_ALT, false), bytes("\x1b\x1b"));
}

#[test]
fn enter_plain_cr() {
    assert_eq!(
        encode_key(&named(NamedKey::Enter), NONE, false),
        bytes("\r")
    );
}

#[test]
fn enter_shift_csiu() {
    assert_eq!(
        encode_key(&named(NamedKey::Enter), SHIFT, false),
        bytes("\x1b[13;2u")
    );
}

#[test]
fn backspace_plain_del() {
    assert_eq!(
        encode_key(&named(NamedKey::Backspace), NONE, false),
        bytes("\x7f")
    );
}

#[test]
fn backspace_ctrl_bs() {
    assert_eq!(
        encode_key(&named(NamedKey::Backspace), CTRL, false),
        bytes("\x08")
    );
}

#[test]
fn arrow_up_plain() {
    assert_eq!(
        encode_key(&named(NamedKey::ArrowUp), NONE, false),
        bytes("\x1b[A")
    );
}

#[test]
fn arrow_up_app_cursor() {
    assert_eq!(
        encode_key(&named(NamedKey::ArrowUp), NONE, true),
        bytes("\x1bOA")
    );
}

#[test]
fn arrow_up_ctrl() {
    assert_eq!(
        encode_key(&named(NamedKey::ArrowUp), CTRL, true),
        bytes("\x1b[1;5A")
    );
}

#[test]
fn tab_shift_backtab() {
    assert_eq!(
        encode_key(&named(NamedKey::Tab), SHIFT, false),
        bytes("\x1b[Z")
    );
}

#[test]
fn home_plain() {
    assert_eq!(
        encode_key(&named(NamedKey::Home), NONE, false),
        bytes("\x1b[H")
    );
}

#[test]
fn home_app_cursor() {
    assert_eq!(
        encode_key(&named(NamedKey::Home), NONE, true),
        bytes("\x1bOH")
    );
}

#[test]
fn home_ctrl_ignores_app_cursor() {
    assert_eq!(
        encode_key(&named(NamedKey::Home), CTRL, true),
        bytes("\x1b[1;5H")
    );
}

#[test]
fn end_plain() {
    assert_eq!(
        encode_key(&named(NamedKey::End), NONE, false),
        bytes("\x1b[F")
    );
}

#[test]
fn end_app_cursor() {
    assert_eq!(
        encode_key(&named(NamedKey::End), NONE, true),
        bytes("\x1bOF")
    );
}

#[test]
fn delete_ctrl_csiu_tilde() {
    assert_eq!(
        encode_key(&named(NamedKey::Delete), CTRL, false),
        bytes("\x1b[3;5~")
    );
}

#[test]
fn alt_char_prefix() {
    assert_eq!(encode_key(&text("a"), ALT, false), bytes("\x1ba"));
}

#[test]
fn f1_plain() {
    assert_eq!(
        encode_key(&named(NamedKey::F1), NONE, false),
        bytes("\x1bOP")
    );
}

#[test]
fn f5_plain() {
    assert_eq!(
        encode_key(&named(NamedKey::F5), NONE, false),
        bytes("\x1b[15~")
    );
}

#[test]
fn f12_plain() {
    assert_eq!(
        encode_key(&named(NamedKey::F12), NONE, false),
        bytes("\x1b[24~")
    );
}

#[test]
fn alt_arrow_up() {
    assert_eq!(
        encode_key(&named(NamedKey::ArrowUp), ALT, true),
        bytes("\x1b[1;3A")
    );
}

#[test]
fn alt_home() {
    assert_eq!(
        encode_key(&named(NamedKey::Home), ALT, true),
        bytes("\x1b[1;3H")
    );
}

#[test]
fn ctrl_pageup() {
    assert_eq!(
        encode_key(&named(NamedKey::PageUp), CTRL, false),
        bytes("\x1b[5;5~")
    );
}

#[test]
fn alt_pageup() {
    assert_eq!(
        encode_key(&named(NamedKey::PageUp), ALT, false),
        bytes("\x1b[5;3~")
    );
}

#[test]
fn function_keys_carry_modifiers() {
    assert_eq!(
        encode_key(&named(NamedKey::F1), CTRL, false),
        bytes("\x1b[1;5P")
    );
    assert_eq!(
        encode_key(&named(NamedKey::F3), SHIFT, false),
        bytes("\x1b[13;2~")
    );
    assert_eq!(
        encode_key(&named(NamedKey::F5), CTRL, false),
        bytes("\x1b[15;5~")
    );
    assert_eq!(
        encode_key(&named(NamedKey::F12), ALT, false),
        bytes("\x1b[24;3~")
    );
}

#[test]
fn insert_carries_modifiers() {
    assert_eq!(
        encode_key(&named(NamedKey::Insert), ALT, false),
        bytes("\x1b[2;3~")
    );
    assert_eq!(
        encode_key(&named(NamedKey::Insert), CTRL, false),
        bytes("\x1b[2;5~")
    );
}

#[test]
fn f13_to_f24_ignore_modifiers() {
    assert_eq!(
        encode_key(&named(NamedKey::F13), CTRL, false),
        bytes("\x1b[1;2P")
    );
    assert_eq!(
        encode_key(&named(NamedKey::F15), ALT, false),
        bytes("\x1b[28~")
    );
}

#[test]
fn alt_prefixes_single_byte_named_keys() {
    assert_eq!(
        encode_key(&named(NamedKey::Enter), ALT, false),
        bytes("\x1b\r")
    );
    assert_eq!(
        encode_key(&named(NamedKey::Tab), ALT, false),
        bytes("\x1b\t")
    );
    assert_eq!(
        encode_key(&named(NamedKey::Tab), SHIFT_ALT, false),
        bytes("\x1b\x1b[Z")
    );
    assert_eq!(
        encode_key(&named(NamedKey::Escape), ALT, false),
        bytes("\x1b\x1b")
    );
    assert_eq!(
        encode_key(&named(NamedKey::Backspace), CTRL_ALT, false),
        bytes("\x1b\x08")
    );
}

#[test]
fn empty_character_sends_nothing() {
    assert_eq!(encode_key(&text(""), NONE, false), None);
    assert_eq!(encode_key(&text(""), ALT, false), None);
    assert_eq!(encode_key(&text(""), CTRL, false), None);
}

#[test]
fn a_repeat_encodes_as_a_press() {
    let mut event = KeyEvent::new(named(NamedKey::ArrowUp), NONE);
    event.kind = KeyEventKind::Repeat;
    assert_eq!(
        super::encode_key_event(&event, &ModeSnapshot::default()),
        bytes("\x1b[A")
    );
}

#[test]
fn an_unrequested_release_sends_nothing() {
    let mut event = KeyEvent::new(text("a"), NONE);
    event.kind = KeyEventKind::Release;
    assert_eq!(
        super::encode_key_event(&event, &ModeSnapshot::default()),
        None
    );
}

#[test]
fn encode_key_is_encode_key_event_on_a_press() {
    let modes = ModeSnapshot::default();
    let key = text("x");
    assert_eq!(
        super::encode_key(&key, ALT, &modes),
        super::encode_key_event(&KeyEvent::new(key.clone(), ALT), &modes)
    );
}
