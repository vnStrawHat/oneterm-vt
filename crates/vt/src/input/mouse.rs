//! Mouse reports: which events a mode reports and the four ways to write one.

use crate::snapshot::{ModeSnapshot, MouseEncoding, MouseProtocol, MouseReporting};

/// A mouse button a program can be told about. Wheel motion has its own encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalMouseButton {
    /// The primary button.
    Left,
    /// The middle button (wheel click).
    Middle,
    /// The secondary button.
    Right,
}

impl TerminalMouseButton {
    fn code(self) -> u8 {
        match self {
            TerminalMouseButton::Left => 0,
            TerminalMouseButton::Middle => 1,
            TerminalMouseButton::Right => 2,
        }
    }
}

/// The modifiers held during a mouse event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseModifiers {
    /// Shift is held (adds 4 to the button code).
    pub shift: bool,
    /// Alt (Option) is held (adds 8).
    pub alt: bool,
    /// Control is held (adds 16).
    pub ctrl: bool,
}

impl MouseModifiers {
    fn mask(self) -> u8 {
        4 * u8::from(self.shift) + 8 * u8::from(self.alt) + 16 * u8::from(self.ctrl)
    }
}

/// Button code of a release outside SGR, and of motion with no button held, before the motion
/// offset.
const NO_BUTTON: u8 = 3;
const MOTION: u8 = 32;
const WHEEL_UP: u8 = 64;
const WHEEL_DOWN: u8 = 65;
/// Added to every button code and coordinate outside SGR and the decimal part of urxvt.
const OFFSET: u8 = 32;

/// The report for a button press at the zero-based viewport cell (`row`, `col`), or an empty
/// `Vec` when the mode does not report it (`modes.mouse` is `None`).
///
/// ```
/// use oneterm_vt::{ModeSnapshot, MouseEncoding, MouseProtocol, MouseReporting};
/// use oneterm_vt::input::{MouseModifiers, TerminalMouseButton, encode_mouse_press};
///
/// let mut modes = ModeSnapshot::default(); // `CSI ? 1000 h` and `CSI ? 1006 h`
/// modes.mouse = Some(MouseProtocol {
///     reporting: MouseReporting::Normal,
///     encoding: MouseEncoding::Sgr,
/// });
/// let press = encode_mouse_press(
///     0, 0, TerminalMouseButton::Left, modes, MouseModifiers::default(),
/// );
/// assert_eq!(press, b"\x1b[<0;1;1M");
/// ```
pub fn encode_mouse_press(
    row: usize,
    col: usize,
    button: TerminalMouseButton,
    modes: ModeSnapshot,
    mods: MouseModifiers,
) -> Vec<u8> {
    let Some(protocol) = modes.mouse else {
        return Vec::new();
    };
    let cell = ViewportCell { row, col };
    write_report(protocol, button.code(), mods, Phase::Down, cell)
}

/// The report for a button release at (`row`, `col`), empty under `?9` and without a mouse mode.
/// Outside SGR a release does not say which button came up.
pub fn encode_mouse_release(
    row: usize,
    col: usize,
    button: TerminalMouseButton,
    modes: ModeSnapshot,
    mods: MouseModifiers,
) -> Vec<u8> {
    let Some(protocol) = modes.mouse else {
        return Vec::new();
    };
    if protocol.reporting == MouseReporting::X10 {
        return Vec::new();
    }
    let code = match protocol.encoding {
        MouseEncoding::Sgr => button.code(),
        MouseEncoding::Default | MouseEncoding::Utf8 | MouseEncoding::Urxvt => NO_BUTTON,
    };
    write_report(protocol, code, mods, Phase::Up, ViewportCell { row, col })
}

/// The report for pointer motion at (`row`, `col`): with `button` held, or `None` for hover.
/// Empty unless the mode reports it: motion with a button needs `?1002` or `?1003`, hover needs
/// `?1003`.
pub fn encode_mouse_move(
    row: usize,
    col: usize,
    button: Option<TerminalMouseButton>,
    modes: ModeSnapshot,
    mods: MouseModifiers,
) -> Vec<u8> {
    let Some(protocol) = modes.mouse else {
        return Vec::new();
    };
    let reported = match protocol.reporting {
        MouseReporting::X10 | MouseReporting::Normal => false,
        MouseReporting::ButtonEvent => button.is_some(),
        MouseReporting::AnyEvent => true,
    };
    if !reported {
        return Vec::new();
    }
    let code = MOTION + button.map_or(NO_BUTTON, TerminalMouseButton::code);
    write_report(protocol, code, mods, Phase::Down, ViewportCell { row, col })
}

/// The report for vertical wheel motion at (`row`, `col`): `delta_y > 0` scrolls up, `< 0` down.
/// Empty for a zero or NaN delta, under `?9` and without a mouse mode.
pub fn encode_wheel_event(
    row: usize,
    col: usize,
    delta_y: f64,
    modes: ModeSnapshot,
    mods: MouseModifiers,
) -> Vec<u8> {
    let Some(protocol) = modes.mouse else {
        return Vec::new();
    };
    if protocol.reporting == MouseReporting::X10 {
        return Vec::new();
    }
    let code = match delta_y.partial_cmp(&0.0) {
        Some(std::cmp::Ordering::Greater) => WHEEL_UP,
        Some(std::cmp::Ordering::Less) => WHEEL_DOWN,
        Some(std::cmp::Ordering::Equal) | None => return Vec::new(),
    };
    write_report(protocol, code, mods, Phase::Down, ViewportCell { row, col })
}

/// Whether SGR ends the report with `M` or `m`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Down,
    Up,
}

/// A zero-based viewport cell.
#[derive(Clone, Copy)]
struct ViewportCell {
    row: usize,
    col: usize,
}

/// Writes the report for button `code` (before modifiers and offsets).
fn write_report(
    protocol: MouseProtocol,
    code: u8,
    mods: MouseModifiers,
    phase: Phase,
    cell: ViewportCell,
) -> Vec<u8> {
    // One-based; saturating so `usize::MAX` neither panics nor wraps.
    let x = cell.col.saturating_add(1);
    let y = cell.row.saturating_add(1);
    if protocol.reporting == MouseReporting::X10 {
        // `?9` predates modifiers and the extended encodings.
        return default_report(code, x, y);
    }
    let code = code + mods.mask();
    match protocol.encoding {
        MouseEncoding::Default => default_report(code, x, y),
        MouseEncoding::Utf8 => utf8_report(code, x, y),
        MouseEncoding::Urxvt => format!("\x1b[{};{x};{y}M", code + OFFSET).into_bytes(),
        MouseEncoding::Sgr => {
            let terminator = if phase == Phase::Up { 'm' } else { 'M' };
            format!("\x1b[<{code};{x};{y}{terminator}").into_bytes()
        }
    }
}

/// `CSI M` and three raw bytes. A coordinate past 223 clamps to 223 (byte `0xff`).
fn default_report(code: u8, x: usize, y: usize) -> Vec<u8> {
    let coordinate =
        |value: usize| u8::try_from(value.saturating_add(32).min(255)).unwrap_or(u8::MAX);
    vec![
        0x1b,
        b'[',
        b'M',
        code + OFFSET,
        coordinate(x),
        coordinate(y),
    ]
}

/// Like [`default_report`] with each coordinate as a UTF-8 scalar; the ceiling is 2015.
fn utf8_report(code: u8, x: usize, y: usize) -> Vec<u8> {
    let mut out = vec![0x1b, b'[', b'M', code + OFFSET];
    for value in [x, y] {
        let scalar = u32::try_from(value.saturating_add(32).min(0x7ff)).ok();
        if let Some(scalar) = scalar.and_then(char::from_u32) {
            out.extend_from_slice(scalar.encode_utf8(&mut [0; 4]).as_bytes());
        }
    }
    out
}

#[cfg(test)]
#[path = "mouse_tests.rs"]
mod mouse_tests;
