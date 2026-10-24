//! Answers to queries: every byte written here is part of the crate's API.
//!
//! Each answer is pushed as a `Reply` event; the embedder writes it to the program verbatim.

use std::borrow::Cow;
use std::fmt::{self, Write as _};

use crate::cell::CellContent;
use crate::graphics::placement::effective_cell;

use super::State;
use super::csi::{Args, DeviceAttributes};
use super::dispatch::Handler;
use super::modes::{CursorShape, Mode};
use super::sgr::sgr_parameters;
use crate::events::VtEvent;

/// Longest DECRQSS or XTGETTCAP payload answered; at the ceiling the request is dropped.
pub(super) const QUERY_MAX_BYTES: usize = 8 * 1024;
/// Names answered per XTGETTCAP request; the rest are dropped.
const XTGETTCAP_MAX_NAMES: usize = 16;
/// Longest XTGETTCAP name, decoded; a longer name is dropped, never cut.
const XTGETTCAP_MAX_NAME_BYTES: usize = 128;
/// Longest product name in bytes.
const PRODUCT_NAME_MAX: usize = 64;
/// The crate version, which XTVERSION and DA2 report unless a product name says otherwise.
const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The DCS request being collected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DcsQuery {
    /// `DCS $ q`: a status string.
    Decrqss,
    /// `DCS + q`: termcap names.
    Xtgettcap,
}

/// Terminfo values for XTGETTCAP, besides `TN`.
const CAPABILITIES: &[(&str, &str)] = &[
    ("Co", "256"),
    ("colors", "256"),
    ("RGB", ""),
    ("Tc", ""),
    ("Su", ""),
    ("setrgbf", "\x1b[38:2::%p1%d:%p2%d:%p3%dm"),
    ("setrgbb", "\x1b[48:2::%p1%d:%p2%d:%p3%dm"),
    ("Ms", "\x1b]52;%p1%s;%p2%s\x07"),
    ("Ss", "\x1b[%p1%d q"),
    ("Se", "\x1b[2 q"),
    ("smcup", "\x1b[?1049h"),
    ("rmcup", "\x1b[?1049l"),
    ("bel", "\x07"),
    ("clear", "\x1b[H\x1b[2J"),
    ("cr", "\r"),
    ("cub1", "\x08"),
    ("cud1", "\n"),
    ("cuf1", "\x1b[C"),
    ("cuu1", "\x1b[A"),
    ("ed", "\x1b[J"),
    ("el", "\x1b[K"),
    ("home", "\x1b[H"),
];

/// Cleans a configured product name: drops C0, DEL and C1 characters, keeps whole characters
/// up to 64 bytes (stopping at the first that does not fit), and treats an empty result as no
/// name.
pub(super) fn sanitize_product_name(name: Option<Cow<'static, str>>) -> Option<Cow<'static, str>> {
    let name = name?;
    let mut clean = String::new();
    for c in name
        .chars()
        .filter(|c| !matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}'))
    {
        if clean.len() + c.len_utf8() > PRODUCT_NAME_MAX {
            break;
        }
        clean.push(c);
    }
    (!clean.is_empty()).then_some(Cow::Owned(clean))
}

/// A decimal number of ASCII digits only.
fn digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `major * 10000 + minor * 100 + patch` of a dotted version; a part that is not a number
/// counts as 0 and every part is capped at 99.
fn version_number(version: &str) -> u32 {
    let mut parts = version
        .split('.')
        .map(|part| digits(part).unwrap_or(0).min(99));
    let mut next = || parts.next().unwrap_or(0);
    let (major, minor, patch) = (next(), next(), next());
    major * 10_000 + minor * 100 + patch
}

/// The version DA2 reports: the one in a `Name(1.2.3)` product name, else the crate's.
fn announced_version(product_name: Option<&str>) -> u32 {
    let from_name = || {
        let inner = product_name?.strip_suffix(')')?.rsplit_once('(')?.1;
        digits(inner.split('.').next()?)?;
        Some(version_number(inner))
    };
    from_name().unwrap_or_else(|| version_number(ENGINE_VERSION))
}

/// The bytes of two hex digits per byte; `None` for odd length or a non-hex digit.
fn decode_hex<'a>(hex: &[u8], buffer: &'a mut [u8; XTGETTCAP_MAX_NAME_BYTES]) -> Option<&'a [u8]> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() / 2 > buffer.len() {
        return None;
    }
    for (slot, pair) in buffer.iter_mut().zip(hex.chunks_exact(2)) {
        let high = char::from(*pair.first()?).to_digit(16)?;
        let low = char::from(*pair.get(1)?).to_digit(16)?;
        *slot = u8::try_from(high * 16 + low).ok()?;
    }
    buffer.get(..hex.len() / 2)
}

/// The value of a termcap name.
fn capability<'a>(name: &[u8], product_name: Option<&'a str>) -> Option<&'a str> {
    if name == b"TN" {
        let product = product_name
            .and_then(|name| name.split('(').next())
            .map(str::trim)
            .filter(|name| !name.is_empty());
        return Some(product.unwrap_or("xterm-256color"));
    }
    CAPABILITIES
        .iter()
        .find(|(candidate, _)| candidate.as_bytes() == name)
        .map(|(_, value)| *value)
}

impl Handler<'_> {
    /// Formats one reply into the scratch buffer and pushes it.
    fn reply(&mut self, args: fmt::Arguments<'_>) {
        let scratch = &mut self.state.scratch;
        scratch.clear();
        // Writing to a `String` cannot fail.
        let _ = scratch.write_fmt(args);
        self.out.push_reply(scratch.as_bytes());
    }

    /// `CSI c`, `CSI > c`, `CSI = c`.
    pub(super) fn device_attributes(&mut self, args: &mut Args<'_>, kind: DeviceAttributes) {
        if args.next_raw().unwrap_or(0) != 0 {
            self.unhandled();
            return;
        }
        match kind {
            DeviceAttributes::Primary => self.reply_primary_attributes(),
            DeviceAttributes::Secondary => {
                let version = announced_version(self.state.config.product_name.as_deref());
                self.reply(format_args!("\x1b[>0;{version};1c"));
            }
            DeviceAttributes::Tertiary => self.reply(format_args!("\x1bP!|00000000\x1b\\")),
        }
    }

    /// DA1 and DECID: a VT220 with Sixel and ANSI colour.
    pub(super) fn reply_primary_attributes(&mut self) {
        self.reply(format_args!("\x1b[?62;4;22c"));
    }

    /// The cursor position as CPR and DECXCPR report it: 1-based, relative to the region top
    /// under DECOM.
    fn cursor_report_position(&mut self) -> (u16, u16) {
        let relative = self.state.modes.get(Mode::Origin);
        let screen = self.state.grid.screen();
        let mut row = screen.cursor_row_index();
        if relative {
            row = row.saturating_sub(screen.region().top);
        }
        (
            row.saturating_add(1),
            screen.cursor().pos.col.saturating_add(1),
        )
    }

    /// `CSI Ps n` and `CSI ? Ps n`.
    pub(super) fn status_report(&mut self, args: &mut Args<'_>, private: bool) {
        match (private, args.next_raw().unwrap_or(0)) {
            (false, 5) => self.reply(format_args!("\x1b[0n")),
            (false, 6) => {
                let (row, col) = self.cursor_report_position();
                self.reply(format_args!("\x1b[{row};{col}R"));
            }
            (true, 6) => {
                let (row, col) = self.cursor_report_position();
                self.reply(format_args!("\x1b[?{row};{col};1R"));
            }
            _ => self.unhandled(),
        }
    }

    /// DECRQM.
    pub(super) fn report_mode(&mut self, args: &mut Args<'_>, private: bool) {
        let number = args.next_raw().unwrap_or(0);
        let mode = if private {
            Mode::from_private(number)
        } else {
            Mode::from_ansi(number)
        };
        let state = mode.map_or(0, |mode| self.state.mode_state(mode) as u8);
        if private {
            self.reply(format_args!("\x1b[?{number};{state}$y"));
        } else {
            self.reply(format_args!("\x1b[{number};{state}$y"));
        }
    }

    /// `CSI > q`: XTVERSION.
    pub(super) fn report_version(&mut self, args: &mut Args<'_>) {
        if args.next_raw().unwrap_or(0) != 0 {
            self.unhandled();
            return;
        }
        let State {
            scratch, config, ..
        } = &mut *self.state;
        scratch.clear();
        // Writing to a `String` cannot fail.
        let _ = match config.product_name.as_deref() {
            Some(name) => write!(scratch, "\x1bP>|{name}\x1b\\"),
            None => write!(scratch, "\x1bP>|oneterm-vt({ENGINE_VERSION})\x1b\\"),
        };
        self.out.push_reply(scratch.as_bytes());
    }

    /// `CSI ? u`.
    pub(super) fn query_keyboard_flags(&mut self) {
        let flags = self.state.keyboard.active.live().bits();
        self.reply(format_args!("\x1b[?{flags}u"));
    }

    /// `CSI > Pp ; Pv m`.
    pub(super) fn set_modify_other_keys(&mut self, args: &mut Args<'_>) {
        let resource = args.next_or(1);
        let level = args.next_or(0);
        match u8::try_from(level) {
            Ok(level) if resource == 4 && level <= 2 => self.state.modify_other_keys = level,
            _ => self.unhandled(),
        }
    }

    /// `CSI ? 4 m`.
    pub(super) fn query_modify_other_keys(&mut self, args: &mut Args<'_>) {
        if args.next_raw().unwrap_or(0) != 4 {
            self.unhandled();
            return;
        }
        let level = self.state.modify_other_keys;
        self.reply(format_args!("\x1b[>4;{level}m"));
    }

    /// `CSI Ps t`: size reports and the title stack.
    pub(super) fn window_operation(&mut self, args: &mut Args<'_>) {
        let (rows, cols) = {
            let screen = self.state.grid.screen();
            (screen.rows(), screen.cols())
        };
        match args.next_raw().unwrap_or(0) {
            14 => {
                let (cell_width, cell_height) = effective_cell(self.state.cell_pixels);
                let height = u32::from(rows) * u32::from(cell_height);
                let width = u32::from(cols) * u32::from(cell_width);
                self.reply(format_args!("\x1b[4;{height};{width}t"));
            }
            18 => self.reply(format_args!("\x1b[8;{rows};{cols}t")),
            22 => self.state.title.push(),
            23 => match self.state.title.pop() {
                Some(Some(title)) => {
                    self.out.push_title(title.as_bytes());
                }
                Some(None) => self.out.push(VtEvent::TitleReset),
                None => {}
            },
            _ => self.unhandled(),
        }
    }

    /// DECRQCRA: the checksum of a rectangle of the visible screen, if the embedder allows.
    pub(super) fn checksum_rectangle(&mut self, args: &mut Args<'_>) {
        let id = args.next_or(0);
        let _page = args.next_raw();
        let screen = self.state.grid.screen();
        let (rows, cols) = (screen.rows(), screen.cols());
        let (min, max) = if self.state.modes.get(Mode::Origin) {
            (screen.region().top, screen.region().bottom)
        } else {
            (0, rows)
        };
        let top_row = args.next_or(1);
        let left_col = args.next_or(1);
        let bottom_row = args.next_or(rows);
        let right_col = args.next_or(cols);
        if !self.state.config.allow_screen_readback {
            self.unhandled();
            return;
        }
        let top = (top_row - 1).saturating_add(min).clamp(min, max);
        let left = (left_col - 1).min(cols);
        let bottom = bottom_row.saturating_add(min).clamp(min, max);
        let right = right_col.min(cols);
        let checksum = rectangle_checksum(self.state, top..bottom, left..right);
        self.reply(format_args!("\x1bP{id}!~{:04X}\x1b\\", checksum & 0xFFFF));
    }

    /// Answers a collected DCS request.
    pub(super) fn answer_dcs_query(&mut self, query: DcsQuery, payload: &[u8]) {
        match query {
            DcsQuery::Decrqss => self.answer_status_string(payload),
            DcsQuery::Xtgettcap => self.answer_termcap(payload),
        }
    }

    fn answer_status_string(&mut self, request: &[u8]) {
        let screen = self.state.grid.screen();
        let value = match request {
            b"m" => {
                let template = screen.cursor().template();
                sgr_parameters(self.state.interner.resolve_style(template.style_id()))
            }
            b"r" => {
                let region = screen.region();
                format!("{};{}", region.top + 1, region.bottom)
            }
            b" q" => {
                let style = self.state.stored_cursor_style();
                let base = match style.shape {
                    CursorShape::Underline => 3,
                    CursorShape::Beam => 5,
                    _ => 1,
                };
                (base + u8::from(!style.blinking)).to_string()
            }
            b"\"q" => "0".to_owned(),
            b"\"p" => "62;1".to_owned(),
            _ => {
                self.reply(format_args!("\x1bP0$r\x1b\\"));
                return;
            }
        };
        // Known requests are ASCII.
        let request = String::from_utf8_lossy(request);
        self.reply(format_args!("\x1bP1$r{value}{request}\x1b\\"));
    }

    fn answer_termcap(&mut self, payload: &[u8]) {
        let mut dropped = false;
        for (index, name) in payload.split(|byte| *byte == b';').enumerate() {
            if index >= XTGETTCAP_MAX_NAMES {
                dropped = true;
                break;
            }
            if name.len() > 2 * XTGETTCAP_MAX_NAME_BYTES {
                dropped = true;
                continue;
            }
            self.answer_capability(name);
        }
        if dropped {
            self.unhandled();
        }
    }

    fn answer_capability(&mut self, hex_name: &[u8]) {
        // The name as sent, when it is hex digits; else nothing.
        let echo = if hex_name.iter().all(u8::is_ascii_hexdigit) {
            std::str::from_utf8(hex_name).unwrap_or_default()
        } else {
            ""
        };
        let mut buffer = [0u8; XTGETTCAP_MAX_NAME_BYTES];
        let State {
            scratch, config, ..
        } = &mut *self.state;
        let value = decode_hex(hex_name, &mut buffer)
            .and_then(|name| capability(name, config.product_name.as_deref()));
        scratch.clear();
        // Writing to a `String` cannot fail.
        match value {
            Some(value) => {
                let _ = write!(scratch, "\x1bP1+r{echo}=");
                for byte in value.bytes() {
                    let _ = write!(scratch, "{byte:02X}");
                }
                scratch.push_str("\x1b\\");
            }
            None => {
                let _ = write!(scratch, "\x1bP0+r{echo}\x1b\\");
            }
        }
        self.out.push_reply(scratch.as_bytes());
    }
}

/// The sum of every scalar shown in the rectangle of the visible screen; a blank counts as
/// `0x20`.
fn rectangle_checksum(
    state: &State,
    rows: std::ops::Range<u16>,
    cols: std::ops::Range<u16>,
) -> u32 {
    let screen = state.grid.screen();
    let graphemes = &state.interner.graphemes;
    let mut sum = 0u32;
    for index in rows {
        let row = screen.row(screen.row_of_index(index));
        for col in cols.clone() {
            match row.cell(col).content() {
                CellContent::Scalar('\t') => sum = sum.wrapping_add(0x20),
                CellContent::Scalar(c) => sum = sum.wrapping_add(u32::from(c)),
                CellContent::Grapheme(id) => {
                    for c in graphemes.resolve(id).chars() {
                        sum = sum.wrapping_add(u32::from(c));
                    }
                }
            }
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_numbers() {
        assert_eq!(version_number("0.1.0"), 100);
        assert_eq!(version_number("1.4.0"), 10_400);
        assert_eq!(version_number("1.4"), 10_400);
        assert_eq!(version_number("7.4294967296.9"), 70_009);
        assert_eq!(version_number("123.456.789"), 99 * 10_000 + 99 * 100 + 99);
        assert_eq!(version_number("1.2.3.4"), 10_203);
        assert_eq!(version_number(""), 0);
        assert_eq!(version_number("+1.2.3"), 203);
    }

    #[test]
    fn announced_versions_fall_back_to_the_engine() {
        let engine = version_number(ENGINE_VERSION);
        assert_eq!(announced_version(None), engine);
        assert_eq!(announced_version(Some("Acme(1.4.0)")), 10_400);
        assert_eq!(announced_version(Some("a(b(2.0.1)")), 20_001);
        for name in [
            "Acme",
            "Acme(1.4.0",
            "Acme()",
            "Acme(-1.2.3)",
            "Acme(+1.2.3)",
        ] {
            assert_eq!(announced_version(Some(name)), engine, "{name}");
        }
    }

    #[test]
    fn hex_names_decode_to_bytes() {
        let mut buffer = [0u8; XTGETTCAP_MAX_NAME_BYTES];
        assert_eq!(decode_hex(b"544e", &mut buffer), Some(&b"TN"[..]));
        assert_eq!(decode_hex(b"544E", &mut buffer), Some(&b"TN"[..]));
        for bad in [&b""[..], b"544", b"54ge", b"zz"] {
            assert_eq!(decode_hex(bad, &mut buffer), None, "{bad:?}");
        }
        let longest = vec![b'a'; 2 * XTGETTCAP_MAX_NAME_BYTES];
        assert_eq!(
            decode_hex(&longest, &mut buffer).map(<[u8]>::len),
            Some(128)
        );
        let too_long = vec![b'a'; 2 * XTGETTCAP_MAX_NAME_BYTES + 2];
        assert_eq!(decode_hex(&too_long, &mut buffer), None);
    }
}
