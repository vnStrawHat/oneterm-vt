//! Rungs 1-3 of the key ladder: unrequested releases, the kitty keyboard protocol and xterm
//! `modifyOtherKeys`.

use std::fmt::Write;

use crate::snapshot::ModeSnapshot;
use crate::terminal::KeyboardFlags;

use super::key::{
    KeyEvent, KeyEventKind, KeySpec, NamedKey, encode_legacy, function_tilde_code, modifier_byte,
};

/// The most scalars of associated text one report carries.
const TEXT_SCALAR_MAX: usize = 32;

/// ASCII punctuation as produced with Shift on a PC-101 layout, and the same keys without it,
/// index for index.
const SHIFTED_ASCII: &[u8] = b"~!@#$%^&*()_+{}|:\"<>?";
const UNSHIFTED_ASCII: &[u8] = b"`1234567890-=[]\\;',./";

/// What the kitty rungs decided.
pub(super) enum Encoded {
    /// The event sends nothing.
    Silent,
    /// The event is these bytes.
    Bytes(Vec<u8>),
    /// No kitty rule applies: ask the next rung.
    Legacy,
}

/// The shape of a key's report (kitty "Functional key definitions").
enum Form {
    /// `CSI code u`.
    CsiU(u32),
    /// `CSI number ~`.
    Tilde(u8),
    /// `CSI letter`, or `CSI 1 ; fields letter`.
    Letter(char),
}

/// Rungs 1 and 2. `ModeSnapshot::keyboard_flags` decides; see the spec's predicates.
pub(super) fn encode(event: &KeyEvent, modes: &ModeSnapshot) -> Encoded {
    let flags = modes.keyboard_flags;
    let report_all = flags.contains(KeyboardFlags::REPORT_ALL_KEYS_AS_ESC);
    let report_types = flags.contains(KeyboardFlags::REPORT_EVENT_TYPES);
    let legacy_c0 = matches!(
        event.key,
        KeySpec::Named(NamedKey::Enter | NamedKey::Tab | NamedKey::Backspace)
    );
    let reportable = report_types && (report_all || !(legacy_c0 || generates_text(event)));

    if event.kind == KeyEventKind::Release && !reportable {
        return Encoded::Silent;
    }
    let applies = !flags.is_empty()
        && (report_all
            || (!legacy_c0 && event.kind != KeyEventKind::Press && reportable)
            || (!legacy_c0
                && flags.contains(KeyboardFlags::DISAMBIGUATE_ESC_CODES)
                && !generates_text(event)));
    if !applies {
        return Encoded::Legacy;
    }
    match report(event, flags) {
        Some(bytes) => Encoded::Bytes(bytes),
        None => Encoded::Silent,
    }
}

/// A text key without Ctrl or Alt: the program gets its text, not an escape code.
fn generates_text(event: &KeyEvent) -> bool {
    matches!(event.key, KeySpec::Character(_)) && !(event.mods.ctrl || event.mods.alt)
}

/// The kitty form of `event`, or `None` when its key has no code.
fn report(event: &KeyEvent, flags: KeyboardFlags) -> Option<Vec<u8>> {
    let form = match &event.key {
        KeySpec::Character(text) => Form::CsiU(unshifted(text)?),
        KeySpec::Named(named) => named_form(*named)?,
    };
    let kind = if flags.contains(KeyboardFlags::REPORT_EVENT_TYPES) {
        match event.kind {
            KeyEventKind::Press => 1,
            KeyEventKind::Repeat => 2,
            KeyEventKind::Release => 3,
        }
    } else {
        1
    };
    let modifiers = modifier_byte(event.mods);
    let mut out = String::from("\x1b[");
    match form {
        Form::CsiU(code) => {
            push_number(&mut out, code);
            if flags.contains(KeyboardFlags::REPORT_ALTERNATE_KEYS) {
                push_alternates(&mut out, event);
            }
            let text = associated_text(event, flags);
            if text.is_some() || modifiers != 1 || kind != 1 {
                out.push(';');
                push_modifiers(&mut out, modifiers, kind);
            }
            if let Some(text) = text {
                out.push(';');
                push_text(&mut out, text);
            }
            out.push('u');
        }
        Form::Tilde(number) => {
            push_number(&mut out, u32::from(number));
            if modifiers != 1 || kind != 1 {
                out.push(';');
                push_modifiers(&mut out, modifiers, kind);
            }
            out.push('~');
        }
        Form::Letter(letter) => {
            if modifiers != 1 || kind != 1 {
                out.push_str("1;");
                push_modifiers(&mut out, modifiers, kind);
            }
            out.push(letter);
        }
    }
    Some(out.into_bytes())
}

fn named_form(key: NamedKey) -> Option<Form> {
    Some(match key {
        NamedKey::Escape => Form::CsiU(27),
        NamedKey::Enter => Form::CsiU(13),
        NamedKey::Tab => Form::CsiU(9),
        NamedKey::Backspace => Form::CsiU(127),
        NamedKey::Insert => Form::Tilde(2),
        NamedKey::Delete => Form::Tilde(3),
        NamedKey::PageUp => Form::Tilde(5),
        NamedKey::PageDown => Form::Tilde(6),
        NamedKey::ArrowUp => Form::Letter('A'),
        NamedKey::ArrowDown => Form::Letter('B'),
        NamedKey::ArrowRight => Form::Letter('C'),
        NamedKey::ArrowLeft => Form::Letter('D'),
        NamedKey::Home => Form::Letter('H'),
        NamedKey::End => Form::Letter('F'),
        function => match function.function_number()? {
            1 => Form::Letter('P'),
            2 => Form::Letter('Q'),
            // The spec removed `CSI R` for F3: it collides with the cursor position report.
            3 => Form::Tilde(13),
            4 => Form::Letter('S'),
            number @ (5..=12) => Form::Tilde(function_tilde_code(number)?),
            // F13 is code 57376 in the private use area, one code per key after it.
            number @ (13..=24) => Form::CsiU(57363 + u32::from(number)),
            _ => return None,
        },
    })
}

/// The code of a `Character`: its first scalar with Shift taken back (PC-101 layout for ASCII
/// punctuation, lower case otherwise). `None` for an empty string.
pub(super) fn unshifted(text: &str) -> Option<u32> {
    let first = text.chars().next()?;
    if let Some(index) = u8::try_from(first)
        .ok()
        .and_then(|byte| SHIFTED_ASCII.iter().position(|&shifted| shifted == byte))
        && let Some(&base) = UNSHIFTED_ASCII.get(index)
    {
        return Some(u32::from(base));
    }
    let lower = first.to_lowercase().next().unwrap_or(first);
    Some(u32::from(lower))
}

/// `:shifted:base` of the `CSI u` form; `shifted` counts only while Shift is held.
fn push_alternates(out: &mut String, event: &KeyEvent) {
    let shifted = event.shifted.filter(|_| event.mods.shift);
    if shifted.is_none() && event.base_layout.is_none() {
        return;
    }
    out.push(':');
    if let Some(shifted) = shifted {
        push_number(out, u32::from(shifted));
    }
    if let Some(base) = event.base_layout {
        out.push(':');
        push_number(out, u32::from(base));
    }
}

/// The text to attach to a `CSI u` report, if it is safe to send.
fn associated_text(event: &KeyEvent, flags: KeyboardFlags) -> Option<&str> {
    if !flags.contains(KeyboardFlags::REPORT_ASSOCIATED_TEXT)
        || !flags.contains(KeyboardFlags::REPORT_ALL_KEYS_AS_ESC)
        || event.kind == KeyEventKind::Release
    {
        return None;
    }
    let text = match (&event.text, &event.key) {
        (Some(text), _) => text.as_str(),
        (None, KeySpec::Character(text)) if generates_text(event) => text.as_str(),
        _ => return None,
    };
    // Bounded count first: a hostile 100 000 scalar string is never scanned whole.
    let within_limit = text.chars().take(TEXT_SCALAR_MAX + 1).count() <= TEXT_SCALAR_MAX;
    let usable = !text.is_empty() && within_limit && !text.chars().any(char::is_control);
    usable.then_some(text)
}

fn push_text(out: &mut String, text: &str) {
    for (index, scalar) in text.chars().enumerate() {
        if index > 0 {
            out.push(':');
        }
        push_number(out, u32::from(scalar));
    }
}

/// `modifiers` and, when it is not a press, `:kind`.
fn push_modifiers(out: &mut String, modifiers: u8, kind: u8) {
    if modifiers == 1 && kind == 1 {
        return;
    }
    push_number(out, u32::from(modifiers));
    if kind != 1 {
        out.push(':');
        push_number(out, u32::from(kind));
    }
}

fn push_number(out: &mut String, number: u32) {
    // Writing to a `String` cannot fail.
    let _ = write!(out, "{number}");
}

/// Rung 3: `CSI 27 ; m ; code ~` for Ctrl or Alt chords, when the program set `modifyOtherKeys`.
pub(super) fn modify_other_keys(event: &KeyEvent, modes: &ModeSnapshot) -> Option<Vec<u8>> {
    let level = modes.modify_other_keys;
    // Shift alone never fires: the layout already consumed it.
    if level == 0 || !(event.mods.ctrl || event.mods.alt) {
        return None;
    }
    let code = match &event.key {
        KeySpec::Character(text) => unshifted(text)?,
        KeySpec::Named(NamedKey::Enter) => 13,
        KeySpec::Named(NamedKey::Tab) => 9,
        KeySpec::Named(NamedKey::Backspace) => 127,
        KeySpec::Named(NamedKey::Escape) => 27,
        // Arrows, Home, the function keys: already unambiguous.
        KeySpec::Named(_) => return None,
    };
    // Level 1 leaves the keys with well-known behaviour alone (Tab, Backspace, Ctrl+Space,
    // Ctrl+3, ...); level 2 and above do not.
    if level == 1 && produces_control_byte(event, modes) {
        return None;
    }
    Some(format!("\x1b[27;{};{code}~", modifier_byte(event.mods)).into_bytes())
}

/// The legacy encoding is a control byte, alone or behind an Alt `ESC`.
fn produces_control_byte(event: &KeyEvent, modes: &ModeSnapshot) -> bool {
    let is_control = |byte: u8| byte < 0x20 || byte == 0x7f;
    match encode_legacy(&event.key, event.mods, modes).as_deref() {
        Some(&[byte]) | Some(&[0x1b, byte]) => is_control(byte),
        _ => false,
    }
}

#[cfg(test)]
#[path = "kitty_tests.rs"]
mod kitty_tests;
