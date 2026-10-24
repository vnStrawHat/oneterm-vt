//! Tests of the kitty keyboard rungs, `modifyOtherKeys` and their interplay with the legacy rung.

use super::*;
use crate::input::{KeyMods, encode_key_event};

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
const SHIFT_ALT: KeyMods = KeyMods {
    shift: true,
    ctrl: false,
    alt: true,
};
const CTRL_ALT: KeyMods = KeyMods {
    shift: false,
    ctrl: true,
    alt: true,
};
const CTRL_SHIFT: KeyMods = KeyMods {
    shift: true,
    ctrl: true,
    alt: false,
};

fn text(text: &str) -> KeySpec {
    KeySpec::Character(text.to_owned())
}

fn named(key: NamedKey) -> KeySpec {
    KeySpec::Named(key)
}

/// Flags from letters: `D` disambiguate, `E` event types, `A` alternate keys, `K` all keys as
/// escape codes, `T` associated text.
fn flags(letters: &str) -> KeyboardFlags {
    let mut flags = KeyboardFlags::empty();
    for letter in letters.chars() {
        flags |= match letter {
            'D' => KeyboardFlags::DISAMBIGUATE_ESC_CODES,
            'E' => KeyboardFlags::REPORT_EVENT_TYPES,
            'A' => KeyboardFlags::REPORT_ALTERNATE_KEYS,
            'K' => KeyboardFlags::REPORT_ALL_KEYS_AS_ESC,
            'T' => KeyboardFlags::REPORT_ASSOCIATED_TEXT,
            other => panic!("unknown flag letter {other}"),
        };
    }
    flags
}

fn modes(letters: &str) -> ModeSnapshot {
    ModeSnapshot {
        keyboard_flags: flags(letters),
        ..ModeSnapshot::default()
    }
}

fn press(key: KeySpec, mods: KeyMods) -> KeyEvent {
    KeyEvent::new(key, mods)
}

fn of_kind(mut event: KeyEvent, kind: KeyEventKind) -> KeyEvent {
    event.kind = kind;
    event
}

fn with_alternates(mut event: KeyEvent, shifted: Option<char>, base: Option<char>) -> KeyEvent {
    event.shifted = shifted;
    event.base_layout = base;
    event
}

fn with_text(mut event: KeyEvent, text: &str) -> KeyEvent {
    event.text = Some(text.to_owned());
    event
}

fn bytes(text: &str) -> Option<Vec<u8>> {
    Some(text.as_bytes().to_vec())
}

fn encode(event: &KeyEvent, letters: &str) -> Option<Vec<u8>> {
    encode_key_event(event, &modes(letters))
}

const ALL_MODS: [KeyMods; 8] = [
    NONE,
    SHIFT,
    CTRL,
    ALT,
    SHIFT_ALT,
    CTRL_SHIFT,
    CTRL_ALT,
    KeyMods {
        shift: true,
        ctrl: true,
        alt: true,
    },
];

/// All 32 combinations of the five flags.
fn all_flag_sets() -> impl Iterator<Item = KeyboardFlags> {
    (0..32).map(KeyboardFlags::from_bits_truncate)
}

/// Every `NamedKey`, from the legacy test's exhaustive list of 38.
fn all_named_keys() -> [NamedKey; 38] {
    use NamedKey::*;
    [
        Enter, Backspace, Delete, Tab, Escape, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home,
        End, PageUp, PageDown, Insert, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14,
        F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    ]
}

/// One row of kitty.md's byte table: the event, the flag letters, the expected bytes.
fn kitty_rows() -> Vec<(KeyEvent, &'static str, Option<Vec<u8>>)> {
    let long_text = "x".repeat(100_000);
    let mut rows = vec![
        (press(named(NamedKey::Escape), NONE), "D", bytes("\x1b[27u")),
        (press(named(NamedKey::Enter), NONE), "K", bytes("\x1b[13u")),
        (press(named(NamedKey::Tab), NONE), "K", bytes("\x1b[9u")),
        (
            press(named(NamedKey::Backspace), NONE),
            "K",
            bytes("\x1b[127u"),
        ),
        (press(named(NamedKey::ArrowUp), NONE), "D", bytes("\x1b[A")),
        (
            press(named(NamedKey::ArrowUp), CTRL_SHIFT),
            "D",
            bytes("\x1b[1;6A"),
        ),
        (
            of_kind(press(named(NamedKey::ArrowUp), NONE), KeyEventKind::Release),
            "E",
            bytes("\x1b[1;1:3A"),
        ),
        (
            press(named(NamedKey::PageUp), CTRL),
            "D",
            bytes("\x1b[5;5~"),
        ),
        (press(text("a"), NONE), "D", bytes("a")),
        (press(text("a"), SHIFT), "D", bytes("a")),
        (press(text("a"), CTRL), "D", bytes("\x1b[97;5u")),
        (press(text("a"), ALT), "D", bytes("\x1b[97;3u")),
        (press(text("a"), SHIFT_ALT), "D", bytes("\x1b[97;4u")),
        (press(text("a"), CTRL_ALT), "D", bytes("\x1b[97;7u")),
        (press(text("i"), CTRL_SHIFT), "D", bytes("\x1b[105;6u")),
        (press(text("A"), CTRL_SHIFT), "D", bytes("\x1b[97;6u")),
        (press(text("!"), CTRL_SHIFT), "D", bytes("\x1b[49;6u")),
        (press(text("a"), NONE), "K", bytes("\x1b[97u")),
        (
            of_kind(press(text("a"), NONE), KeyEventKind::Repeat),
            "EK",
            bytes("\x1b[97;1:2u"),
        ),
        (
            of_kind(press(text("a"), NONE), KeyEventKind::Release),
            "EK",
            bytes("\x1b[97;1:3u"),
        ),
        (
            with_text(press(text("a"), SHIFT), "A"),
            "KT",
            bytes("\x1b[97;2;65u"),
        ),
        (
            with_alternates(press(text("a"), SHIFT), Some('A'), None),
            "KA",
            bytes("\x1b[97:65;2u"),
        ),
        (
            with_alternates(press(text("a"), CTRL), None, Some('a')),
            "KA",
            bytes("\x1b[97::97;5u"),
        ),
        (
            with_alternates(press(text("a"), CTRL), Some('A'), None),
            "KA",
            bytes("\x1b[97;5u"),
        ),
        (
            with_alternates(press(text("a"), SHIFT), Some('A'), Some('a')),
            "KA",
            bytes("\x1b[97:65:97;2u"),
        ),
        (
            with_text(press(text("a"), NONE), "\u{1}"),
            "KT",
            bytes("\x1b[97u"),
        ),
        (
            with_text(press(text("a"), NONE), "a\u{7f}"),
            "KT",
            bytes("\x1b[97u"),
        ),
        (
            with_text(press(text("a"), NONE), "a\u{9b}b"),
            "KT",
            bytes("\x1b[97u"),
        ),
        (
            with_text(press(text("a"), NONE), &long_text),
            "KT",
            bytes("\x1b[97u"),
        ),
        (
            with_text(press(text("a"), NONE), "ab"),
            "KT",
            bytes("\x1b[97;;97:98u"),
        ),
        (press(text("a"), NONE), "KT", bytes("\x1b[97;;97u")),
        (press(text("a"), SHIFT), "KT", bytes("\x1b[97;2;97u")),
        (press(text("a"), CTRL), "KT", bytes("\x1b[97;5u")),
        (press(text("a"), ALT), "KT", bytes("\x1b[97;3u")),
        (press(text("a"), CTRL_ALT), "KT", bytes("\x1b[97;7u")),
        (
            with_text(press(text("a"), ALT), "å"),
            "KT",
            bytes("\x1b[97;3;229u"),
        ),
        (press(named(NamedKey::F13), NONE), "D", bytes("\x1b[57376u")),
        (press(named(NamedKey::F15), NONE), "D", bytes("\x1b[57378u")),
        (press(named(NamedKey::F17), NONE), "D", bytes("\x1b[57380u")),
        (press(named(NamedKey::F24), NONE), "D", bytes("\x1b[57387u")),
        (press(named(NamedKey::F3), NONE), "K", bytes("\x1b[13~")),
        (press(text(" "), CTRL), "", bytes("\x00")),
    ];
    for key in [NamedKey::Enter, NamedKey::Tab, NamedKey::Backspace] {
        let legacy = match key {
            NamedKey::Enter => "\r",
            NamedKey::Tab => "\t",
            _ => "\x7f",
        };
        let code = match key {
            NamedKey::Enter => "13",
            NamedKey::Tab => "9",
            _ => "127",
        };
        for letters in ["D", "DE", "DEAT"] {
            rows.push((press(named(key), NONE), letters, bytes(legacy)));
        }
        let release = of_kind(press(named(key), NONE), KeyEventKind::Release);
        rows.push((release.clone(), "E", None));
        rows.push((release, "EK", bytes(&format!("\x1b[{code};1:3u"))));
    }
    for letters in ["E", "DE", "EA"] {
        rows.push((
            of_kind(press(text("a"), NONE), KeyEventKind::Repeat),
            letters,
            bytes("a"),
        ));
        rows.push((
            of_kind(press(text("a"), NONE), KeyEventKind::Release),
            letters,
            None,
        ));
    }
    for letters in ["A", "T", "AT"] {
        let event = with_text(
            with_alternates(press(text("a"), NONE), Some('A'), Some('a')),
            "a",
        );
        rows.push((event, letters, bytes("a")));
    }
    rows
}

#[test]
fn kitty_table() {
    for (index, (event, letters, want)) in kitty_rows().into_iter().enumerate() {
        // DECCKM is on for every row: the table's `ArrowUp` + `app_cursor` row, and proof that no
        // other row reads it.
        let mut snapshot = modes(letters);
        snapshot.app_cursor = true;
        let got = encode_key_event(&event, &snapshot);
        assert_eq!(got, want, "row {index}: {event:?} flags {letters:?}");
    }
}

/// One row of modify-other-keys.md's byte table, at levels 0, 1 and 2.
fn modify_other_keys_rows() -> Vec<(KeySpec, KeyMods, [Option<&'static str>; 3])> {
    vec![
        (
            text("a"),
            CTRL,
            [Some("\x01"), Some("\x01"), Some("\x1b[27;5;97~")],
        ),
        (
            text("2"),
            CTRL,
            [Some("\x00"), Some("\x00"), Some("\x1b[27;5;50~")],
        ),
        (
            text("3"),
            CTRL,
            [Some("\x1b"), Some("\x1b"), Some("\x1b[27;5;51~")],
        ),
        (
            text(" "),
            CTRL,
            [Some("\x00"), Some("\x00"), Some("\x1b[27;5;32~")],
        ),
        (
            text(";"),
            CTRL,
            [Some(";"), Some("\x1b[27;5;59~"), Some("\x1b[27;5;59~")],
        ),
        (
            text("9"),
            CTRL,
            [Some("9"), Some("\x1b[27;5;57~"), Some("\x1b[27;5;57~")],
        ),
        (
            text("é"),
            CTRL,
            [None, Some("\x1b[27;5;233~"), Some("\x1b[27;5;233~")],
        ),
        (
            named(NamedKey::Enter),
            CTRL,
            [
                Some("\x1b[13;5u"),
                Some("\x1b[27;5;13~"),
                Some("\x1b[27;5;13~"),
            ],
        ),
        (
            named(NamedKey::Tab),
            CTRL,
            [Some("\t"), Some("\t"), Some("\x1b[27;5;9~")],
        ),
        (
            named(NamedKey::Backspace),
            CTRL,
            [Some("\x08"), Some("\x08"), Some("\x1b[27;5;127~")],
        ),
        (
            named(NamedKey::Escape),
            CTRL,
            [Some("\x1b"), Some("\x1b"), Some("\x1b[27;5;27~")],
        ),
        (
            named(NamedKey::ArrowUp),
            CTRL,
            [Some("\x1b[1;5A"), Some("\x1b[1;5A"), Some("\x1b[1;5A")],
        ),
        (
            text("a"),
            ALT,
            [Some("\x1ba"), Some("\x1b[27;3;97~"), Some("\x1b[27;3;97~")],
        ),
        (
            named(NamedKey::Enter),
            ALT,
            [Some("\x1b\r"), Some("\x1b\r"), Some("\x1b[27;3;13~")],
        ),
        (
            text("A"),
            CTRL_SHIFT,
            [Some("\x01"), Some("\x01"), Some("\x1b[27;6;97~")],
        ),
        (text("A"), NONE, [Some("A"), Some("A"), Some("A")]),
        (text("A"), SHIFT, [Some("A"), Some("A"), Some("A")]),
    ]
}

#[test]
fn modify_other_keys_table() {
    for (key, mods, wants) in modify_other_keys_rows() {
        for (level, want) in wants.into_iter().enumerate() {
            let snapshot = ModeSnapshot {
                modify_other_keys: level as u8,
                ..ModeSnapshot::default()
            };
            let got = crate::input::encode_key(&key, mods, &snapshot);
            assert_eq!(got, want.and_then(bytes), "{key:?} {mods:?} level {level}");
        }
    }
}

#[test]
fn specification_worked_examples() {
    // kitty keyboard protocol, "Disambiguate escape codes" and "Report event types".
    assert_eq!(
        encode(&press(named(NamedKey::Escape), NONE), "D"),
        bytes("\x1b[27u")
    );
    assert_eq!(
        encode(&press(text("a"), CTRL_ALT), "D"),
        bytes("\x1b[97;7u")
    );
    assert_eq!(
        encode(&press(named(NamedKey::F5), CTRL), "D"),
        bytes("\x1b[15;5~")
    );
    let release = of_kind(press(named(NamedKey::ArrowUp), NONE), KeyEventKind::Release);
    assert_eq!(encode(&release, "DE"), bytes("\x1b[1;1:3A"));
    let shifted = with_alternates(press(text("a"), SHIFT), Some('A'), None);
    assert_eq!(encode(&shifted, "KA"), bytes("\x1b[97:65;2u"));
}

#[test]
fn enter_tab_and_backspace_survive_a_crashed_program() {
    for letters in ["D", "DE", "DEA", "DEAT"] {
        let enter = |mods| encode(&press(named(NamedKey::Enter), mods), letters);
        let tab = |mods| encode(&press(named(NamedKey::Tab), mods), letters);
        let backspace = |mods| encode(&press(named(NamedKey::Backspace), mods), letters);
        assert_eq!(enter(NONE), bytes("\r"), "{letters}");
        assert_eq!(tab(NONE), bytes("\t"), "{letters}");
        assert_eq!(backspace(NONE), bytes("\x7f"), "{letters}");
        assert_eq!(enter(CTRL), bytes("\x1b[13;5u"), "{letters}");
        assert_eq!(tab(CTRL), bytes("\t"), "{letters}");
        assert_eq!(backspace(CTRL), bytes("\x08"), "{letters}");
    }
}

#[test]
fn the_three_legacy_keys_have_no_release_without_report_all() {
    for key in [NamedKey::Enter, NamedKey::Tab, NamedKey::Backspace] {
        let release = of_kind(press(named(key), NONE), KeyEventKind::Release);
        assert_eq!(encode(&release, "E"), None, "{key:?} E");
        assert_eq!(encode(&release, "DE"), None, "{key:?} DE");
        assert!(encode(&release, "EK").is_some(), "{key:?} EK");
    }
}

#[test]
fn release_is_silent_and_repeat_is_a_press_by_default() {
    let release = of_kind(press(text("a"), NONE), KeyEventKind::Release);
    let repeat = of_kind(press(text("a"), NONE), KeyEventKind::Repeat);
    assert_eq!(encode(&release, ""), None);
    assert_eq!(encode(&repeat, ""), bytes("a"));
    let up_repeat = of_kind(press(named(NamedKey::ArrowUp), NONE), KeyEventKind::Repeat);
    assert_eq!(encode(&up_repeat, ""), bytes("\x1b[A"));
    // Without `REPORT_EVENT_TYPES` a repeat under `D` is still a plain press.
    assert_eq!(encode(&up_repeat, "D"), bytes("\x1b[A"));
}

#[test]
fn app_cursor_is_not_read_once_the_kitty_rung_applies() {
    let snapshot = ModeSnapshot {
        app_cursor: true,
        keyboard_flags: flags("D"),
        ..ModeSnapshot::default()
    };
    let up = press(named(NamedKey::ArrowUp), NONE);
    assert_eq!(encode_key_event(&up, &snapshot), bytes("\x1b[A"));
    let home = press(named(NamedKey::Home), NONE);
    assert_eq!(encode_key_event(&home, &snapshot), bytes("\x1b[H"));
}

#[test]
fn disambiguate_covers_only_the_keys_that_produce_no_text() {
    assert_eq!(encode(&press(text("a"), NONE), "D"), bytes("a"));
    assert_eq!(encode(&press(text("A"), SHIFT), "D"), bytes("A"));
    assert_eq!(encode(&press(text("é"), NONE), "D"), bytes("é"));
    assert_eq!(encode(&press(text("a"), CTRL), "D"), bytes("\x1b[97;5u"));
    assert_eq!(
        encode(&press(named(NamedKey::Escape), NONE), "D"),
        bytes("\x1b[27u")
    );
    assert_eq!(
        encode(&press(named(NamedKey::Delete), NONE), "D"),
        bytes("\x1b[3~")
    );
}

#[test]
fn alternate_keys_and_text_are_inert_on_their_own() {
    let event = with_text(
        with_alternates(press(text("a"), SHIFT), Some('A'), Some('a')),
        "A",
    );
    for letters in ["A", "T", "AT"] {
        assert_eq!(encode(&event, letters), bytes("a"), "flags {letters}");
        assert_eq!(
            encode(&press(named(NamedKey::ArrowUp), NONE), letters),
            bytes("\x1b[A"),
            "flags {letters}"
        );
    }
    // The text field also needs `REPORT_ALL_KEYS_AS_ESC`, so `D` + `T` stays text.
    assert_eq!(encode(&event, "DT"), bytes("a"));
}

#[test]
fn the_text_field_is_dropped_rather_than_sent_unsafely() {
    for payload in ["\u{1}", "a\u{7f}", "a\u{9b}b", "\x1b[2J", ""] {
        let event = with_text(press(text("a"), NONE), payload);
        assert_eq!(encode(&event, "KT"), bytes("\x1b[97u"), "{payload:?}");
    }
    let scalars_32 = "x".repeat(32);
    let kept = with_text(press(text("a"), NONE), &scalars_32);
    let got = encode(&kept, "KT").unwrap_or_default();
    assert!(got.ends_with(b"120u"), "32 scalars are kept");
    let scalars_33 = "x".repeat(33);
    let dropped = with_text(press(text("a"), NONE), &scalars_33);
    assert_eq!(encode(&dropped, "KT"), bytes("\x1b[97u"));
}

#[test]
fn an_empty_character_has_no_kitty_encoding() {
    for set in all_flag_sets() {
        for mods in [NONE, ALT, CTRL] {
            for kind in [
                KeyEventKind::Press,
                KeyEventKind::Repeat,
                KeyEventKind::Release,
            ] {
                let snapshot = ModeSnapshot {
                    keyboard_flags: set,
                    modify_other_keys: 2,
                    ..ModeSnapshot::default()
                };
                let event = of_kind(press(text(""), mods), kind);
                assert_eq!(
                    encode_key_event(&event, &snapshot),
                    None,
                    "{set:?} {mods:?}"
                );
            }
        }
    }
}

#[test]
fn modify_other_keys_levels() {
    let at = |level: u8, key: KeySpec, mods: KeyMods| {
        let snapshot = ModeSnapshot {
            modify_other_keys: level,
            ..ModeSnapshot::default()
        };
        crate::input::encode_key(&key, mods, &snapshot)
    };
    assert_eq!(at(0, text("a"), CTRL), bytes("\x01"));
    assert_eq!(at(1, text("a"), CTRL), bytes("\x01"));
    assert_eq!(at(2, text("a"), CTRL), bytes("\x1b[27;5;97~"));
    // Shift alone never fires; neither do named keys without a code.
    assert_eq!(at(2, text("A"), SHIFT), bytes("A"));
    assert_eq!(at(2, named(NamedKey::F5), CTRL), bytes("\x1b[15;5~"));
    // A hand-built level above 2 acts as level 2.
    assert_eq!(at(3, text("a"), CTRL), bytes("\x1b[27;5;97~"));
}

#[test]
fn kitty_wins_over_modify_other_keys() {
    let snapshot = ModeSnapshot {
        keyboard_flags: flags("D"),
        modify_other_keys: 2,
        ..ModeSnapshot::default()
    };
    let event = press(text("a"), CTRL);
    assert_eq!(encode_key_event(&event, &snapshot), bytes("\x1b[97;5u"));
    // A flag that does not apply leaves `modifyOtherKeys` reachable.
    let snapshot = ModeSnapshot {
        keyboard_flags: flags("A"),
        modify_other_keys: 2,
        ..ModeSnapshot::default()
    };
    assert_eq!(encode_key_event(&event, &snapshot), bytes("\x1b[27;5;97~"));
}

#[test]
fn shifted_function_keys_take_the_private_use_codes() {
    let keys = [
        NamedKey::F13,
        NamedKey::F14,
        NamedKey::F15,
        NamedKey::F16,
        NamedKey::F17,
        NamedKey::F18,
        NamedKey::F19,
        NamedKey::F20,
        NamedKey::F21,
        NamedKey::F22,
        NamedKey::F23,
        NamedKey::F24,
    ];
    for (offset, key) in keys.into_iter().enumerate() {
        let want = format!("\x1b[{}u", 57376 + offset);
        assert_eq!(
            encode(&press(named(key), NONE), "D"),
            bytes(&want),
            "{key:?}"
        );
    }
}

#[test]
fn f3_never_collides_with_the_cursor_position_report() {
    for set in all_flag_sets() {
        for mods in ALL_MODS {
            let snapshot = ModeSnapshot {
                keyboard_flags: set,
                ..ModeSnapshot::default()
            };
            let got = crate::input::encode_key(&named(NamedKey::F3), mods, &snapshot);
            let Some(got) = got else { continue };
            assert!(!is_csi_ending_in_r(&got), "{set:?} {mods:?}: {got:?}");
        }
    }
}

fn is_csi_ending_in_r(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x1b[") && bytes.ends_with(b"R")
}

#[test]
fn no_output_is_a_cursor_position_report() {
    let kinds = [
        KeyEventKind::Press,
        KeyEventKind::Repeat,
        KeyEventKind::Release,
    ];
    let mut keys: Vec<KeySpec> = all_named_keys().into_iter().map(named).collect();
    keys.extend(["a", "R", "é"].map(text));
    for key in &keys {
        for mods in ALL_MODS {
            for set in all_flag_sets() {
                for level in 0..=2 {
                    for kind in kinds {
                        let snapshot = ModeSnapshot {
                            keyboard_flags: set,
                            modify_other_keys: level,
                            ..ModeSnapshot::default()
                        };
                        let event = of_kind(press(key.clone(), mods), kind);
                        if let Some(got) = encode_key_event(&event, &snapshot) {
                            assert!(
                                !is_csi_ending_in_r(&got),
                                "{key:?} {mods:?} {set:?} level {level} {kind:?}: {got:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_text_key_has_no_event_types_without_report_all_keys() {
    let repeat = of_kind(press(text("a"), NONE), KeyEventKind::Repeat);
    assert_eq!(encode(&repeat, "DE"), bytes("a"));
    assert_eq!(encode(&press(text("a"), NONE), "DE"), bytes("a"));
    let release = of_kind(press(text("a"), NONE), KeyEventKind::Release);
    assert_eq!(encode(&release, "DE"), None);
}

#[test]
fn the_text_fallback_respects_the_modifiers() {
    assert_eq!(encode(&press(text("a"), NONE), "KT"), bytes("\x1b[97;;97u"));
    assert_eq!(
        encode(&press(text("a"), SHIFT), "KT"),
        bytes("\x1b[97;2;97u")
    );
    assert_eq!(encode(&press(text("a"), CTRL), "KT"), bytes("\x1b[97;5u"));
    assert_eq!(encode(&press(text("a"), ALT), "KT"), bytes("\x1b[97;3u"));
    let release = of_kind(press(text("a"), NONE), KeyEventKind::Release);
    assert_eq!(encode(&release, "EKT"), bytes("\x1b[97;1:3u"));
}

#[test]
fn unshifted_follows_the_pc101_layout_and_lowercases() {
    assert_eq!(unshifted("!"), Some(u32::from('1')));
    assert_eq!(unshifted("~"), Some(u32::from('`')));
    assert_eq!(unshifted("?"), Some(u32::from('/')));
    assert_eq!(unshifted("A"), Some(u32::from('a')));
    assert_eq!(unshifted("Éa"), Some(u32::from('é')));
    assert_eq!(unshifted("ab"), Some(u32::from('a')));
    assert_eq!(unshifted(""), None);
}
