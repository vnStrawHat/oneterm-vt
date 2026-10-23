//! Colours a program sets with OSC 4, 10, 11, 12, 17 and 19, and the sequences that reset them.
//!
//! The engine keeps only this override layer. The theme underneath, and turning a cell's
//! colour into pixels, belong to the embedder.

use crate::cell::Rgb;
use crate::events::VtEvent;
use crate::parser::{OscParams, StringTerm};

use super::dispatch::Handler;

/// Slots in [`ColorOverrides`]: 256 palette entries, three dynamic colours, a gap, and the two
/// selection colours.
pub(super) const COLOR_COUNT: usize = 271;

/// A colour a program can set, query or reset.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ColorKey {
    /// An entry of the 256-colour palette (OSC 4, reset by OSC 104).
    Palette(u8),
    /// The default foreground (OSC 10, reset by OSC 110).
    Foreground,
    /// The default background (OSC 11, reset by OSC 111).
    Background,
    /// The cursor colour (OSC 12, reset by OSC 112).
    Cursor,
    /// The selection background (OSC 17, reset only by a full reset).
    SelectionBackground,
    /// The selection foreground (OSC 19, reset only by a full reset).
    SelectionForeground,
}

impl ColorKey {
    /// The slot of the key in [`ColorOverrides`]: palette entries 0..=255, then 256
    /// foreground, 257 background, 258 cursor, 269 selection background, 270 selection
    /// foreground. The values are stable.
    pub const fn index(self) -> usize {
        match self {
            ColorKey::Palette(n) => n as usize,
            ColorKey::Foreground => 256,
            ColorKey::Background => 257,
            ColorKey::Cursor => 258,
            ColorKey::SelectionBackground => 269,
            ColorKey::SelectionForeground => 270,
        }
    }

    /// The key of a slot; `None` outside `0..271` and in the unused gap `259..=268`.
    pub const fn from_index(index: usize) -> Option<ColorKey> {
        match index {
            0..=255 => Some(ColorKey::Palette(index as u8)),
            256 => Some(ColorKey::Foreground),
            257 => Some(ColorKey::Background),
            258 => Some(ColorKey::Cursor),
            269 => Some(ColorKey::SelectionBackground),
            270 => Some(ColorKey::SelectionForeground),
            _ => None,
        }
    }

    /// The number an OSC colour reply starts with: `4;<n>` for a palette entry, else the OSC
    /// number of the colour.
    pub fn query_prefix(self) -> String {
        match self {
            ColorKey::Palette(n) => format!("4;{n}"),
            ColorKey::Foreground => "10".to_owned(),
            ColorKey::Background => "11".to_owned(),
            ColorKey::Cursor => "12".to_owned(),
            ColorKey::SelectionBackground => "17".to_owned(),
            ColorKey::SelectionForeground => "19".to_owned(),
        }
    }

    /// The bytes that answer a [`VtEvent::ColorQuery`] for this key:
    /// `ESC ] <prefix> ; rgb:rrrr/gggg/bbbb <terminator>` with each 8-bit channel written as
    /// two identical lower-case hex pairs, `BEL` or `ESC \` as the terminator.
    pub fn color_reply(self, color: Rgb, terminator: StringTerm) -> Vec<u8> {
        let end = match terminator {
            StringTerm::Bel => "\x07",
            StringTerm::St => "\x1b\\",
        };
        let Rgb { r, g, b } = color;
        format!(
            "\x1b]{};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}{end}",
            self.query_prefix()
        )
        .into_bytes()
    }
}

/// The colours programs changed with OSC sequences.
///
/// A slot is `None` while the embedder's theme owns the colour.
#[derive(Clone, Debug)]
pub struct ColorOverrides {
    slots: Box<[Option<Rgb>; COLOR_COUNT]>,
}

impl Default for ColorOverrides {
    fn default() -> Self {
        ColorOverrides {
            slots: Box::new([None; COLOR_COUNT]),
        }
    }
}

impl ColorOverrides {
    /// The override of `key`, if a program set one.
    pub fn get(&self, key: ColorKey) -> Option<Rgb> {
        self.slots.get(key.index()).copied().flatten()
    }

    /// Every set slot as `(ColorKey::index, colour)`, ascending.
    pub fn iter(&self) -> impl Iterator<Item = (usize, Rgb)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.map(|color| (index, color)))
    }

    /// Stores an override; returns whether the slot changed.
    pub(super) fn set(&mut self, key: ColorKey, color: Rgb) -> bool {
        self.write(key.index(), Some(color))
    }

    /// Removes an override; returns whether the slot changed.
    pub(super) fn reset(&mut self, key: ColorKey) -> bool {
        self.write(key.index(), None)
    }

    /// Removes the 256 palette overrides; returns whether any changed.
    pub(super) fn reset_palette(&mut self) -> bool {
        (0..=255).fold(false, |changed, index| self.write(index, None) | changed)
    }

    /// Removes every override; returns whether any changed.
    pub(super) fn reset_all(&mut self) -> bool {
        (0..COLOR_COUNT).fold(false, |changed, index| self.write(index, None) | changed)
    }

    fn write(&mut self, index: usize, value: Option<Rgb>) -> bool {
        match self.slots.get_mut(index) {
            Some(slot) if *slot != value => {
                *slot = value;
                true
            }
            _ => false,
        }
    }
}

/// Parses a colour spec: `#` and 3, 6, 9 or 12 hex digits, or `rgb:R/G/B` with 1-4 hex digits
/// per channel. Names, `rgbi:` and anything malformed give `None`.
pub(super) fn parse_color(spec: &[u8]) -> Option<Rgb> {
    if let Some(digits) = spec.strip_prefix(b"#") {
        if digits.is_empty() || digits.len() % 3 != 0 || digits.len() > 12 {
            return None;
        }
        let width = digits.len() / 3;
        let mut channels = digits.chunks_exact(width);
        // The first two digits of each group are the channel; one digit is the high nibble.
        let mut channel = || {
            let group = channels.next()?;
            let high = hex_digit(*group.first()?)?;
            let low = match group.get(1) {
                Some(&digit) => hex_digit(digit)?,
                None => 0,
            };
            // Every digit of the group must be hex.
            group
                .iter()
                .all(u8::is_ascii_hexdigit)
                .then_some((high << 4) | low)
        };
        return Some(Rgb {
            r: channel()?,
            g: channel()?,
            b: channel()?,
        });
    }
    let rest = spec.strip_prefix(b"rgb:")?;
    let mut parts = rest.split(|byte| *byte == b'/');
    let r = scaled_channel(parts.next()?)?;
    let g = scaled_channel(parts.next()?)?;
    let b = scaled_channel(parts.next()?)?;
    parts.next().is_none().then_some(Rgb { r, g, b })
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).map(|digit| digit as u8)
}

/// One `rgb:` channel of 1-4 hex digits, scaled to `0..=255`.
fn scaled_channel(digits: &[u8]) -> Option<u8> {
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    let value = digits.iter().try_fold(0u32, |value, byte| {
        hex_digit(*byte).map(|digit| (value << 4) | u32::from(digit))
    })?;
    let max = (1u32 << (4 * digits.len())) - 1;
    u8::try_from(255 * value / max).ok()
}

/// A palette index: a non-empty decimal run that fits `u8`.
fn parse_index(text: &[u8]) -> Option<u8> {
    if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse().ok()
}

/// The OSC parameters after the code, each split on `;` (only the last parser parameter can
/// hold one), without allocating.
fn args<'a>(params: &'a OscParams<'a>) -> impl Iterator<Item = &'a [u8]> + 'a {
    params
        .iter()
        .skip(1)
        .flat_map(|param| param.split(|byte| *byte == b';'))
}

impl Handler<'_> {
    /// One `(key, spec)` pair: sets the colour, queries it with `?`, or counts a bad spec.
    fn color_value(&mut self, key: ColorKey, spec: &[u8], term: StringTerm) {
        if spec == b"?" {
            self.out.push(VtEvent::ColorQuery {
                key,
                terminator: term,
            });
        } else if let Some(color) = parse_color(spec) {
            let changed = self.state.colors.set(key, color);
            self.state.note_palette_change(changed);
        } else {
            self.unhandled();
        }
    }

    /// OSC 4.
    pub(super) fn osc_palette(&mut self, params: &OscParams<'_>, term: StringTerm) {
        let mut pairs = args(params);
        if args(params).nth(1).is_none() {
            self.unhandled();
            return;
        }
        while let Some(index) = pairs.next() {
            // A trailing odd parameter is ignored.
            let Some(spec) = pairs.next() else {
                break;
            };
            match parse_index(index) {
                Some(n) => self.color_value(ColorKey::Palette(n), spec, term),
                None => self.unhandled(),
            }
        }
    }

    /// OSC 10, 11 and 12: each parameter sets the next colour in that order.
    pub(super) fn osc_dynamic(&mut self, code: u32, params: &OscParams<'_>, term: StringTerm) {
        let mut specs = args(params).peekable();
        if specs.peek().is_none() {
            self.unhandled();
            return;
        }
        for (spec, number) in specs.zip(code..) {
            let key = match number {
                10 => ColorKey::Foreground,
                11 => ColorKey::Background,
                12 => ColorKey::Cursor,
                _ => {
                    self.unhandled();
                    break;
                }
            };
            self.color_value(key, spec, term);
        }
    }

    /// OSC 17 and 19: one colour; a further parameter is counted.
    pub(super) fn osc_selection_color(
        &mut self,
        key: ColorKey,
        params: &OscParams<'_>,
        term: StringTerm,
    ) {
        match params.get(1) {
            Some(spec) if !spec.is_empty() => self.color_value(key, spec, term),
            _ => self.unhandled(),
        }
        if params.len() > 2 {
            self.unhandled();
        }
    }

    /// OSC 104: resets the listed palette entries, or all of them without a list.
    pub(super) fn osc_reset_palette(&mut self, params: &OscParams<'_>) {
        let mut indexes = args(params).peekable();
        if indexes.peek().is_none_or(|first| first.is_empty()) {
            let changed = self.state.colors.reset_palette();
            self.state.note_palette_change(changed);
            return;
        }
        for index in indexes {
            match parse_index(index) {
                Some(n) => {
                    let changed = self.state.colors.reset(ColorKey::Palette(n));
                    self.state.note_palette_change(changed);
                }
                None => self.unhandled(),
            }
        }
    }

    /// OSC 110, 111 and 112.
    pub(super) fn osc_reset_color(&mut self, key: ColorKey) {
        let changed = self.state.colors.reset(key);
        self.state.note_palette_change(changed);
    }
}
