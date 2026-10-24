//! Tests of the mouse encoders: the byte table, the reporting filter and the coordinate ceilings.

use super::*;

const NO_MODS: MouseModifiers = MouseModifiers {
    shift: false,
    alt: false,
    ctrl: false,
};
const CTRL: MouseModifiers = MouseModifiers {
    shift: false,
    alt: false,
    ctrl: true,
};
const SHIFT: MouseModifiers = MouseModifiers {
    shift: true,
    alt: false,
    ctrl: false,
};
const SHIFT_ALT: MouseModifiers = MouseModifiers {
    shift: true,
    alt: true,
    ctrl: false,
};
const SHIFT_CTRL: MouseModifiers = MouseModifiers {
    shift: true,
    alt: false,
    ctrl: true,
};

use MouseEncoding::{Default as Plain, Sgr, Urxvt, Utf8};
use MouseReporting::{AnyEvent, ButtonEvent, Normal, X10};
use TerminalMouseButton::{Left, Middle, Right};

fn snapshot(reporting: MouseReporting, encoding: MouseEncoding) -> ModeSnapshot {
    ModeSnapshot {
        mouse: Some(MouseProtocol {
            reporting,
            encoding,
        }),
        ..ModeSnapshot::default()
    }
}

/// One call of an encoder.
#[derive(Clone, Copy, Debug)]
enum Call {
    Press(usize, usize, TerminalMouseButton, MouseModifiers),
    Release(usize, usize, TerminalMouseButton, MouseModifiers),
    Move(usize, usize, Option<TerminalMouseButton>, MouseModifiers),
    Wheel(usize, usize, f64, MouseModifiers),
}

impl Call {
    fn run(self, modes: ModeSnapshot) -> Vec<u8> {
        match self {
            Call::Press(row, col, button, mods) => {
                encode_mouse_press(row, col, button, modes, mods)
            }
            Call::Release(row, col, button, mods) => {
                encode_mouse_release(row, col, button, modes, mods)
            }
            Call::Move(row, col, button, mods) => encode_mouse_move(row, col, button, modes, mods),
            Call::Wheel(row, col, delta, mods) => encode_wheel_event(row, col, delta, modes, mods),
        }
    }
}

fn press(row: usize, col: usize) -> Call {
    Call::Press(row, col, Left, NO_MODS)
}

/// mouse.md's byte table, row for row.
fn mouse_rows() -> Vec<(Call, Option<MouseProtocol>, &'static [u8])> {
    let protocol = |reporting, encoding| {
        Some(MouseProtocol {
            reporting,
            encoding,
        })
    };
    let x10 = protocol(X10, Plain);
    let normal_sgr = protocol(Normal, Sgr);
    let normal = protocol(Normal, Plain);
    let any = |encoding| protocol(AnyEvent, encoding);
    vec![
        (press(0, 0), x10, b"\x1b[M\x20\x21\x21"),
        (Call::Press(0, 0, Left, CTRL), x10, b"\x1b[M\x20\x21\x21"),
        (Call::Release(0, 0, Left, NO_MODS), x10, b""),
        (Call::Move(0, 0, None, NO_MODS), x10, b""),
        (Call::Move(0, 0, Some(Left), NO_MODS), x10, b""),
        (Call::Wheel(0, 0, 1.0, NO_MODS), x10, b""),
        (press(0, 0), None, b""),
        (Call::Release(0, 0, Left, NO_MODS), None, b""),
        (Call::Move(0, 0, Some(Left), NO_MODS), None, b""),
        (Call::Move(0, 0, None, NO_MODS), None, b""),
        (Call::Wheel(0, 0, 1.0, NO_MODS), None, b""),
        (press(0, 0), normal_sgr, b"\x1b[<0;1;1M"),
        (
            Call::Release(2, 3, Left, NO_MODS),
            normal_sgr,
            b"\x1b[<0;4;3m",
        ),
        (Call::Press(0, 0, Left, CTRL), normal_sgr, b"\x1b[<16;1;1M"),
        (
            Call::Release(3, 7, Right, SHIFT_CTRL),
            normal_sgr,
            b"\x1b[<22;8;4m",
        ),
        (
            Call::Press(0, 0, Middle, NO_MODS),
            normal_sgr,
            b"\x1b[<1;1;1M",
        ),
        (
            Call::Wheel(5, 5, 1.0, NO_MODS),
            normal_sgr,
            b"\x1b[<64;6;6M",
        ),
        (
            Call::Wheel(5, 5, -1.0, NO_MODS),
            normal_sgr,
            b"\x1b[<65;6;6M",
        ),
        (Call::Wheel(5, 5, 0.0, NO_MODS), normal_sgr, b""),
        (Call::Wheel(5, 5, f64::NAN, NO_MODS), normal_sgr, b""),
        (Call::Move(0, 0, None, NO_MODS), any(Sgr), b"\x1b[<35;1;1M"),
        (press(500, 1000), normal_sgr, b"\x1b[<0;1001;501M"),
        (press(200, 200), normal_sgr, b"\x1b[<0;201;201M"),
        (press(299, 299), normal_sgr, b"\x1b[<0;300;300M"),
        (press(0, 0), normal, b"\x1b[M\x20\x21\x21"),
        (
            Call::Release(0, 0, Left, NO_MODS),
            normal,
            b"\x1b[M\x23\x21\x21",
        ),
        (Call::Press(0, 0, Left, CTRL), normal, b"\x1b[M0!!"),
        (Call::Press(5, 10, Right, SHIFT_ALT), normal, b"\x1b[M.+&"),
        (press(94, 94), normal, b"\x1b[M\x20\x7f\x7f"),
        (press(200, 200), normal, b"\x1b[M\x20\xe9\xe9"),
        (press(500, 500), normal, b"\x1b[M\x20\xff\xff"),
        (press(222, 222), normal, b"\x1b[M\x20\xff\xff"),
        (press(usize::MAX, usize::MAX), normal, b"\x1b[M\x20\xff\xff"),
        (Call::Wheel(5, 5, 1.0, NO_MODS), normal, b"\x1b[M`&&"),
        (Call::Move(2, 3, None, NO_MODS), any(Plain), b"\x1b[MC$#"),
        (press(0, 0), protocol(Normal, Utf8), b"\x1b[M\x20\x21\x21"),
        (
            press(200, 200),
            protocol(Normal, Utf8),
            b"\x1b[M\x20\xc3\xa9\xc3\xa9",
        ),
        (
            press(299, 299),
            protocol(Normal, Utf8),
            b"\x1b[M\x20\xc5\x8c\xc5\x8c",
        ),
        (
            press(5000, 5000),
            protocol(Normal, Utf8),
            b"\x1b[M\x20\xdf\xbf\xdf\xbf",
        ),
        (
            press(usize::MAX, usize::MAX),
            protocol(Normal, Utf8),
            b"\x1b[M\x20\xdf\xbf\xdf\xbf",
        ),
        (
            press(299, 299),
            protocol(Normal, Urxvt),
            b"\x1b[32;300;300M",
        ),
        (
            Call::Release(2, 3, Left, NO_MODS),
            protocol(Normal, Urxvt),
            b"\x1b[35;4;3M",
        ),
        (
            Call::Press(0, 0, Right, SHIFT),
            protocol(Normal, Urxvt),
            b"\x1b[38;1;1M",
        ),
        (Call::Move(2, 3, None, NO_MODS), any(Urxvt), b"\x1b[67;4;3M"),
        (Call::Move(2, 3, Some(Left), NO_MODS), normal_sgr, b""),
        (
            Call::Move(2, 3, Some(Left), NO_MODS),
            protocol(ButtonEvent, Sgr),
            b"\x1b[<32;4;3M",
        ),
        (
            Call::Move(2, 3, Some(Left), NO_MODS),
            any(Sgr),
            b"\x1b[<32;4;3M",
        ),
        (Call::Move(2, 3, None, NO_MODS), normal_sgr, b""),
        (
            Call::Move(2, 3, None, NO_MODS),
            protocol(ButtonEvent, Sgr),
            b"",
        ),
        (press(0, 0), protocol(X10, Sgr), b"\x1b[M\x20\x21\x21"),
        (
            press(usize::MAX, usize::MAX),
            normal_sgr,
            b"\x1b[<0;18446744073709551615;18446744073709551615M",
        ),
    ]
}

#[test]
fn mouse_table() {
    for (index, (call, mouse, want)) in mouse_rows().into_iter().enumerate() {
        let modes = ModeSnapshot {
            mouse,
            ..ModeSnapshot::default()
        };
        assert_eq!(call.run(modes), want, "row {index}: {call:?} {mouse:?}");
    }
}

#[test]
fn x10_reports_a_press_and_nothing_else() {
    let modes = snapshot(X10, Plain);
    assert_eq!(
        Call::Press(0, 0, Left, NO_MODS).run(modes),
        b"\x1b[M\x20\x21\x21"
    );
    assert!(Call::Release(0, 0, Left, NO_MODS).run(modes).is_empty());
    assert!(Call::Move(0, 0, Some(Left), NO_MODS).run(modes).is_empty());
    assert!(Call::Move(0, 0, None, NO_MODS).run(modes).is_empty());
    assert!(Call::Wheel(0, 0, 1.0, NO_MODS).run(modes).is_empty());
}

#[test]
fn x10_ignores_the_encoding() {
    let want = b"\x1b[M\x20\x21\x21";
    for encoding in [Plain, Utf8, Sgr, Urxvt] {
        let modes = snapshot(X10, encoding);
        assert_eq!(press(0, 0).run(modes), want, "{encoding:?}");
        assert_eq!(
            Call::Press(0, 0, Left, SHIFT_CTRL).run(modes),
            want,
            "{encoding:?}"
        );
    }
}

#[test]
fn urxvt_is_decimal_and_has_no_column_ceiling() {
    let modes = snapshot(Normal, Urxvt);
    let got = press(299, 299).run(modes);
    assert_eq!(got, b"\x1b[32;300;300M");
    assert!(got.iter().all(|byte| *byte < 0x80));
    assert_eq!(press(9999, 9999).run(modes), b"\x1b[32;10000;10000M");
    // D4: a hover is reported only under `?1003`.
    assert_eq!(
        Call::Move(2, 3, None, NO_MODS).run(snapshot(AnyEvent, Urxvt)),
        b"\x1b[67;4;3M"
    );
}

#[test]
fn sgr_press_left() {
    assert_eq!(press(0, 0).run(snapshot(Normal, Sgr)), b"\x1b[<0;1;1M");
}

#[test]
fn sgr_release_left() {
    let got = Call::Release(2, 3, Left, NO_MODS).run(snapshot(Normal, Sgr));
    assert_eq!(got, b"\x1b[<0;4;3m");
}

#[test]
fn sgr_press_with_ctrl() {
    let got = Call::Press(0, 0, Left, CTRL).run(snapshot(Normal, Sgr));
    assert_eq!(got, b"\x1b[<16;1;1M");
}

#[test]
fn x11_press_left() {
    assert_eq!(
        press(0, 0).run(snapshot(Normal, Plain)),
        b"\x1b[M\x20\x21\x21"
    );
}

#[test]
fn x11_release_uses_button_3() {
    let got = Call::Release(0, 0, Left, NO_MODS).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M\x23\x21\x21");
    let got = Call::Release(0, 0, Right, NO_MODS).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M\x23\x21\x21");
}

#[test]
fn wheel_up_sgr() {
    let got = Call::Wheel(5, 5, 1.0, NO_MODS).run(snapshot(Normal, Sgr));
    assert_eq!(got, b"\x1b[<64;6;6M");
}

#[test]
fn wheel_down_sgr() {
    let got = Call::Wheel(5, 5, -1.0, NO_MODS).run(snapshot(Normal, Sgr));
    assert_eq!(got, b"\x1b[<65;6;6M");
}

#[test]
fn move_hover_code_35() {
    let got = Call::Move(0, 0, None, NO_MODS).run(snapshot(AnyEvent, Sgr));
    assert_eq!(got, b"\x1b[<35;1;1M");
}

#[test]
fn x11_press_with_ctrl() {
    let got = Call::Press(0, 0, Left, CTRL).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M0!!");
}

#[test]
fn x11_press_with_shift_alt() {
    let got = Call::Press(5, 10, Right, SHIFT_ALT).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M.+&");
}

#[test]
fn x11_coordinates_above_127_are_raw_bytes() {
    let got = press(200, 200).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M\x20\xe9\xe9");
}

#[test]
fn x11_coordinates_beyond_byte_range_are_capped() {
    let got = press(500, 500).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M\x20\xff\xff");
}

#[test]
fn utf8_mouse_encodes_large_coordinates_as_utf8() {
    let got = press(200, 200).run(snapshot(Normal, Utf8));
    assert_eq!(got, b"\x1b[M\x20\xc3\xa9\xc3\xa9");
}

#[test]
fn utf8_mouse_small_coordinates_match_legacy() {
    let legacy = press(10, 20).run(snapshot(Normal, Plain));
    assert_eq!(press(10, 20).run(snapshot(Normal, Utf8)), legacy);
}

#[test]
fn utf8_mouse_caps_at_two_byte_form() {
    let got = press(5000, 5000).run(snapshot(Normal, Utf8));
    assert_eq!(got, b"\x1b[M\x20\xdf\xbf\xdf\xbf");
}

#[test]
fn sgr_wins_over_utf8() {
    // One protocol carries one encoding; an SGR protocol never writes raw bytes.
    let got = press(200, 200).run(snapshot(Normal, Sgr));
    assert_eq!(got, b"\x1b[<0;201;201M");
}

#[test]
fn x11_coordinates_at_boundary_95() {
    let got = press(94, 94).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M\x20\x7f\x7f");
}

#[test]
fn sgr_preserves_large_coordinates() {
    let got = press(500, 1000).run(snapshot(Normal, Sgr));
    assert_eq!(got, b"\x1b[<0;1001;501M");
}

#[test]
fn sgr_release_right_with_modifiers() {
    let got = Call::Release(3, 7, Right, SHIFT_CTRL).run(snapshot(Normal, Sgr));
    assert_eq!(got, b"\x1b[<22;8;4m");
}

#[test]
fn wheel_x11_mode() {
    let got = Call::Wheel(5, 5, 1.0, NO_MODS).run(snapshot(Normal, Plain));
    assert_eq!(got, b"\x1b[M`&&");
}

#[test]
fn move_hover_x11() {
    let got = Call::Move(2, 3, None, NO_MODS).run(snapshot(AnyEvent, Plain));
    assert_eq!(got, b"\x1b[MC$#");
}

#[test]
fn motion_follows_reporting() {
    let rows = [
        (X10, false, false),
        (Normal, false, false),
        (ButtonEvent, true, false),
        (AnyEvent, true, true),
    ];
    for (reporting, drag, hover) in rows {
        let modes = snapshot(reporting, Sgr);
        let dragged = Call::Move(2, 3, Some(Left), NO_MODS).run(modes);
        let hovered = Call::Move(2, 3, None, NO_MODS).run(modes);
        assert_eq!(!dragged.is_empty(), drag, "{reporting:?} drag");
        assert_eq!(!hovered.is_empty(), hover, "{reporting:?} hover");
    }
}

#[test]
fn wheel_without_vertical_delta_sends_nothing() {
    let modes = snapshot(Normal, Sgr);
    for delta in [0.0, -0.0, f64::NAN] {
        assert!(
            Call::Wheel(1, 1, delta, NO_MODS).run(modes).is_empty(),
            "{delta}"
        );
    }
}
