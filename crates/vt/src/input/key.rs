//! Key events, the ladder and the legacy (xterm) encoding.

use crate::snapshot::ModeSnapshot;

use super::kitty::{self, Encoded};

const ESC: u8 = 0x1b;

/// The modifiers held with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods {
    /// Shift is held.
    pub shift: bool,
    /// Control is held.
    pub ctrl: bool,
    /// Alt (Option) is held.
    pub alt: bool,
}

/// A key that does not produce text by itself. Keypad, lock, media and modifier keys have no
/// variant: they are not encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NamedKey {
    /// Enter (Return).
    Enter,
    /// Backspace.
    Backspace,
    /// Forward delete.
    Delete,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Cursor up.
    ArrowUp,
    /// Cursor down.
    ArrowDown,
    /// Cursor left.
    ArrowLeft,
    /// Cursor right.
    ArrowRight,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Insert.
    Insert,
    /// Function key 1.
    F1,
    /// Function key 2.
    F2,
    /// Function key 3.
    F3,
    /// Function key 4.
    F4,
    /// Function key 5.
    F5,
    /// Function key 6.
    F6,
    /// Function key 7.
    F7,
    /// Function key 8.
    F8,
    /// Function key 9.
    F9,
    /// Function key 10.
    F10,
    /// Function key 11.
    F11,
    /// Function key 12.
    F12,
    /// Function key 13.
    F13,
    /// Function key 14.
    F14,
    /// Function key 15.
    F15,
    /// Function key 16.
    F16,
    /// Function key 17.
    F17,
    /// Function key 18.
    F18,
    /// Function key 19.
    F19,
    /// Function key 20.
    F20,
    /// Function key 21.
    F21,
    /// Function key 22.
    F22,
    /// Function key 23.
    F23,
    /// Function key 24.
    F24,
}

impl NamedKey {
    /// `1..=24` for `F1..=F24`, `None` for every other key.
    pub(super) fn function_number(self) -> Option<u8> {
        match self {
            NamedKey::Enter
            | NamedKey::Backspace
            | NamedKey::Delete
            | NamedKey::Tab
            | NamedKey::Escape
            | NamedKey::ArrowUp
            | NamedKey::ArrowDown
            | NamedKey::ArrowLeft
            | NamedKey::ArrowRight
            | NamedKey::Home
            | NamedKey::End
            | NamedKey::PageUp
            | NamedKey::PageDown
            | NamedKey::Insert => None,
            NamedKey::F1 => Some(1),
            NamedKey::F2 => Some(2),
            NamedKey::F3 => Some(3),
            NamedKey::F4 => Some(4),
            NamedKey::F5 => Some(5),
            NamedKey::F6 => Some(6),
            NamedKey::F7 => Some(7),
            NamedKey::F8 => Some(8),
            NamedKey::F9 => Some(9),
            NamedKey::F10 => Some(10),
            NamedKey::F11 => Some(11),
            NamedKey::F12 => Some(12),
            NamedKey::F13 => Some(13),
            NamedKey::F14 => Some(14),
            NamedKey::F15 => Some(15),
            NamedKey::F16 => Some(16),
            NamedKey::F17 => Some(17),
            NamedKey::F18 => Some(18),
            NamedKey::F19 => Some(19),
            NamedKey::F20 => Some(20),
            NamedKey::F21 => Some(21),
            NamedKey::F22 => Some(22),
            NamedKey::F23 => Some(23),
            NamedKey::F24 => Some(24),
        }
    }
}

/// What was pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeySpec {
    /// A key that produced text: the text the key produced with the layout and Shift already
    /// applied (`"a"`, `"A"`, `"é"`). An empty string sends nothing.
    Character(String),
    /// A key that does not produce text.
    Named(NamedKey),
}

/// Whether the key went down, repeated or came up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum KeyEventKind {
    /// The key went down.
    #[default]
    Press,
    /// The key is held and the system repeated it.
    Repeat,
    /// The key came up. Sent only when the program asked for it (kitty `REPORT_EVENT_TYPES`).
    Release,
}

/// One key event with everything the kitty keyboard protocol can report.
///
/// Build it with [`KeyEvent::new`] and assign the fields you know; the struct is
/// `#[non_exhaustive]`, so a new field is not a breaking change.
///
/// ```
/// use oneterm_vt::input::{KeyEvent, KeyEventKind, KeyMods, KeySpec, NamedKey};
///
/// let mut event = KeyEvent::new(KeySpec::Named(NamedKey::Escape), KeyMods::default());
/// event.kind = KeyEventKind::Release;
/// assert_eq!(event.text, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyEvent {
    /// The key.
    pub key: KeySpec,
    /// The modifiers held.
    pub mods: KeyMods,
    /// Press, repeat or release.
    pub kind: KeyEventKind,
    /// The character the key produces with Shift held. Sent (kitty `REPORT_ALTERNATE_KEYS`)
    /// only when Shift is held.
    pub shifted: Option<char>,
    /// The character the key has on a PC-101 layout at the same physical position. Sent with
    /// `REPORT_ALTERNATE_KEYS`.
    pub base_layout: Option<char>,
    /// The text the event produced, sent with `REPORT_ASSOCIATED_TEXT`. `None` falls back to the
    /// [`KeySpec::Character`] payload when neither Ctrl nor Alt is held.
    pub text: Option<String>,
}

impl KeyEvent {
    /// A press of `key` with `mods`; the other fields are `None`.
    pub fn new(key: KeySpec, mods: KeyMods) -> KeyEvent {
        KeyEvent {
            key,
            mods,
            kind: KeyEventKind::Press,
            shifted: None,
            base_layout: None,
            text: None,
        }
    }
}

/// The bytes for a key press, or `None` when nothing is to be sent. The same as
/// [`encode_key_event`] on [`KeyEvent::new`].
///
/// ```
/// use oneterm_vt::ModeSnapshot;
/// use oneterm_vt::input::{KeyMods, KeySpec, encode_key};
///
/// let ctrl = KeyMods { ctrl: true, ..KeyMods::default() };
/// let bytes = encode_key(&KeySpec::Character("c".into()), ctrl, &ModeSnapshot::default());
/// assert_eq!(bytes.as_deref(), Some(&b"\x03"[..]));
/// ```
pub fn encode_key(key: &KeySpec, mods: KeyMods, modes: &ModeSnapshot) -> Option<Vec<u8>> {
    encode_key_event(&KeyEvent::new(key.clone(), mods), modes)
}

/// The bytes for a key event against `modes`, or `None` when the event sends nothing (a release
/// nobody asked for, an empty [`KeySpec::Character`], a Ctrl chord with no encoding such as
/// Ctrl+`é`); the embedder drops it. A `Some` is never empty.
///
/// The first rung that applies wins: kitty keyboard flags, then `modifyOtherKeys`, then the
/// legacy table.
///
/// ```
/// use oneterm_vt::{KeyboardFlags, ModeSnapshot};
/// use oneterm_vt::input::{KeyEvent, KeyMods, KeySpec, NamedKey, encode_key_event};
///
/// let escape = KeyEvent::new(KeySpec::Named(NamedKey::Escape), KeyMods::default());
/// let mut modes = ModeSnapshot::default();
/// assert_eq!(encode_key_event(&escape, &modes).as_deref(), Some(&b"\x1b"[..]));
///
/// // The program sent `CSI > 1 u`: Escape is now unambiguous.
/// modes.keyboard_flags = KeyboardFlags::DISAMBIGUATE_ESC_CODES;
/// assert_eq!(encode_key_event(&escape, &modes).as_deref(), Some(&b"\x1b[27u"[..]));
/// ```
pub fn encode_key_event(event: &KeyEvent, modes: &ModeSnapshot) -> Option<Vec<u8>> {
    if matches!(&event.key, KeySpec::Character(text) if text.is_empty()) {
        return None;
    }
    match kitty::encode(event, modes) {
        Encoded::Silent => return None,
        Encoded::Bytes(bytes) => return Some(bytes),
        Encoded::Legacy => {}
    }
    if let Some(bytes) = kitty::modify_other_keys(event, modes) {
        return Some(bytes);
    }
    encode_legacy(&event.key, event.mods, modes)
}

/// `1 + shift + 2*alt + 4*ctrl`, the modifier parameter of every CSI key form.
pub(super) fn modifier_byte(mods: KeyMods) -> u8 {
    1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl)
}

/// The tilde code of `F5..=F12` and `F17..=F24` (`CSI code ~`).
pub(super) fn function_tilde_code(number: u8) -> Option<u8> {
    const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
    let index = match number {
        5..=12 => number - 5,
        17..=24 => number - 17,
        _ => return None,
    };
    CODES.get(usize::from(index)).copied()
}

/// Rung 4: the legacy (xterm) table. Reads only `app_cursor`; a repeat is a press.
pub(super) fn encode_legacy(key: &KeySpec, mods: KeyMods, modes: &ModeSnapshot) -> Option<Vec<u8>> {
    match key {
        KeySpec::Character(text) => legacy_character(text, mods),
        KeySpec::Named(named) => legacy_named(*named, mods, modes.app_cursor),
    }
}

fn legacy_character(text: &str, mods: KeyMods) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() + 1);
    if mods.alt {
        out.push(ESC);
    }
    if mods.ctrl {
        out.push(ctrl_byte(text)?);
    } else {
        out.extend_from_slice(text.as_bytes());
    }
    Some(out)
}

/// The control byte of Ctrl plus exactly one ASCII scalar; `None` for anything else.
fn ctrl_byte(text: &str) -> Option<u8> {
    let mut chars = text.chars();
    let scalar = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let byte = u8::try_from(scalar).ok().filter(u8::is_ascii)?;
    Some(match byte {
        b'a'..=b'z' | b'A'..=b'Z' => byte & 0x1f,
        b' ' | b'2' | b'@' => 0x00,
        b'3' | b'[' => 0x1b,
        b'4' | b'\\' => 0x1c,
        b'5' | b']' => 0x1d,
        b'6' | b'^' | b'~' => 0x1e,
        b'7' | b'_' | b'/' => 0x1f,
        b'8' | b'?' => 0x7f,
        other => other,
    })
}

/// `CSI code ~`, or `CSI code ; m ~` with modifiers.
fn csi_tilde(code: u8, m: u8) -> Vec<u8> {
    if m > 1 {
        format!("\x1b[{code};{m}~").into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

/// How an unmodified cursor-style key is introduced.
#[derive(Clone, Copy)]
enum PlainForm {
    Csi,
    Ss3,
}

/// A cursor-style key: plain `CSI letter` or `SS3 letter` per `plain`, `CSI 1 ; m letter`
/// with modifiers.
fn csi_letter(letter: char, m: u8, plain: PlainForm) -> Vec<u8> {
    match (m > 1, plain) {
        (true, _) => format!("[1;{m}{letter}").into_bytes(),
        (false, PlainForm::Ss3) => format!("O{letter}").into_bytes(),
        (false, PlainForm::Csi) => format!("[{letter}").into_bytes(),
    }
}

fn legacy_named(key: NamedKey, mods: KeyMods, app_cursor: bool) -> Option<Vec<u8>> {
    let m = modifier_byte(mods);
    let plain = if app_cursor {
        PlainForm::Ss3
    } else {
        PlainForm::Csi
    };
    let bytes = match key {
        NamedKey::Enter if mods.shift || mods.ctrl => format!("\x1b[13;{m}u").into_bytes(),
        NamedKey::Enter => alt_prefixed(mods, b"\r"),
        NamedKey::Backspace => alt_prefixed(mods, &[if mods.ctrl { 0x08 } else { 0x7f }]),
        NamedKey::Delete => csi_tilde(3, m),
        NamedKey::Tab if mods.shift => alt_prefixed(mods, b"\x1b[Z"),
        NamedKey::Tab => alt_prefixed(mods, b"\t"),
        NamedKey::Escape => alt_prefixed(mods, &[ESC]),
        NamedKey::ArrowUp => csi_letter('A', m, plain),
        NamedKey::ArrowDown => csi_letter('B', m, plain),
        NamedKey::ArrowRight => csi_letter('C', m, plain),
        NamedKey::ArrowLeft => csi_letter('D', m, plain),
        NamedKey::Home => csi_letter('H', m, plain),
        NamedKey::End => csi_letter('F', m, plain),
        NamedKey::PageUp => csi_tilde(5, m),
        NamedKey::PageDown => csi_tilde(6, m),
        NamedKey::Insert => csi_tilde(2, m),
        function => return legacy_function(function.function_number()?, m),
    };
    Some(bytes)
}

/// `bytes`, with `ESC` in front when Alt is held.
fn alt_prefixed(mods: KeyMods, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    if mods.alt {
        out.push(ESC);
    }
    out.extend_from_slice(bytes);
    out
}

/// F1-F12 carry the modifier; F13-F24 are the fixed shifted forms. F3 is never `CSI 1 ; m R`:
/// that is byte-identical to a Cursor Position Report, and F15 avoids it the same way.
fn legacy_function(number: u8, m: u8) -> Option<Vec<u8>> {
    let bytes = match number {
        1 => csi_letter('P', m, PlainForm::Ss3),
        2 => csi_letter('Q', m, PlainForm::Ss3),
        3 if m > 1 => csi_tilde(13, m),
        3 => b"\x1bOR".to_vec(),
        4 => csi_letter('S', m, PlainForm::Ss3),
        5..=12 => csi_tilde(function_tilde_code(number)?, m),
        13 => b"\x1b[1;2P".to_vec(),
        14 => b"\x1b[1;2Q".to_vec(),
        15 => b"\x1b[28~".to_vec(),
        16 => b"\x1b[1;2S".to_vec(),
        17..=24 => csi_tilde(function_tilde_code(number)?, 2),
        _ => return None,
    };
    Some(bytes)
}

#[cfg(test)]
#[path = "key_tests.rs"]
mod key_tests;
