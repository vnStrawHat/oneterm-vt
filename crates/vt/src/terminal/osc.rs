//! OSC dispatch: routing by number and every built-in handler.

use crate::cell::Semantic;
use crate::events::{ClipboardKind, Progress, ShellMark, VtEvent};
use crate::intern::{Extras, HyperlinkId};
use crate::parser::{OscParams, StringTerm};

use super::color::ColorKey;
use super::dispatch::Handler;
use super::modes::{CursorShape, CursorStyle};
use super::osc_routes::OscRoute;

impl Handler<'_> {
    pub(super) fn osc_dispatch(
        &mut self,
        code: Option<u32>,
        params: &OscParams<'_>,
        term: StringTerm,
        truncated: bool,
    ) {
        if truncated {
            let stats = &mut self.state.stats;
            stats.truncated_osc = stats.truncated_osc.saturating_add(1);
        }
        let Some(code) = code else {
            self.unhandled();
            return;
        };
        let route = self.state.config.osc_routes.get(code);
        if route == OscRoute::Drop {
            self.unhandled();
            return;
        }
        if route != OscRoute::Forward {
            self.builtin_osc(code, params, term, truncated);
        }
        if route != OscRoute::Builtin {
            self.out.push_osc_params(code, params, term, truncated);
        }
    }

    fn builtin_osc(
        &mut self,
        code: u32,
        params: &OscParams<'_>,
        term: StringTerm,
        truncated: bool,
    ) {
        match code {
            0 | 2 => self.osc_title(params),
            1 => self.osc_icon_name(params),
            4 => self.osc_palette(params, term),
            7 => self.osc_cwd(params, truncated),
            8 => self.osc_hyperlink(params),
            9 => self.osc_notification(params, truncated),
            10..=12 => self.osc_dynamic(code, params, term),
            17 => self.osc_selection_color(ColorKey::SelectionBackground, params, term),
            19 => self.osc_selection_color(ColorKey::SelectionForeground, params, term),
            22 => self.osc_pointer(params),
            50 => self.osc_cursor_shape(params),
            52 => self.osc_clipboard(params),
            104 => self.osc_reset_palette(params),
            110 => self.osc_reset_color(ColorKey::Foreground),
            111 => self.osc_reset_color(ColorKey::Background),
            112 => self.osc_reset_color(ColorKey::Cursor),
            133 => self.osc_shell_mark(params),
            _ => self.unhandled(),
        }
    }

    /// Joins parameters `from..` with `;` into the arena, skipping those that are not UTF-8,
    /// and returns the trimmed text.
    fn trimmed_text(&mut self, params: &OscParams<'_>, from: usize) -> crate::events::StrSpan {
        let mark = self.out.mark();
        let mut first = true;
        for param in params.iter().skip(from) {
            if std::str::from_utf8(param).is_err() {
                continue;
            }
            if !first {
                self.out.extend(b";");
            }
            first = false;
            self.out.extend(param);
        }
        self.out.finish_trimmed(mark).unwrap_or_else(|| {
            // Every piece was checked, so this only guards the arena limit.
            self.out.text_span("")
        })
    }

    /// OSC 0 and 2.
    fn osc_title(&mut self, params: &OscParams<'_>) {
        if params.len() < 2 {
            self.unhandled();
            return;
        }
        let span = self.trimmed_text(params, 1);
        let text = self.out.str(span);
        if text.is_empty() {
            self.state.title.title = None;
            self.out.push(VtEvent::TitleReset);
        } else {
            self.state.title.title = Some(text.to_owned());
            self.out.push(VtEvent::Title(span));
        }
    }

    /// OSC 1.
    fn osc_icon_name(&mut self, params: &OscParams<'_>) {
        if params.len() < 2 {
            self.unhandled();
            return;
        }
        let span = self.trimmed_text(params, 1);
        self.out.push(VtEvent::IconName(span));
    }

    /// Parameters `from..` joined with `;` as strict UTF-8 into the scratch buffer.
    fn join_strict(&mut self, params: &OscParams<'_>, from: usize) -> bool {
        self.state.scratch.clear();
        for (index, param) in params.iter().enumerate().skip(from) {
            let Ok(text) = std::str::from_utf8(param) else {
                return false;
            };
            if index > from {
                self.state.scratch.push(';');
            }
            self.state.scratch.push_str(text);
        }
        true
    }

    /// Parameters `from..` joined with `;` as lossy UTF-8 into the scratch buffer.
    fn join_lossy(&mut self, params: &OscParams<'_>, from: usize) {
        self.state.scratch.clear();
        for (index, param) in params.iter().enumerate().skip(from) {
            if index > from {
                self.state.scratch.push(';');
            }
            self.state.scratch.push_str(&String::from_utf8_lossy(param));
        }
    }

    /// OSC 7.
    fn osc_cwd(&mut self, params: &OscParams<'_>, truncated: bool) {
        if truncated || !self.join_strict(params, 1) {
            self.unhandled();
            return;
        }
        let (host, path) = split_cwd(&self.state.scratch);
        let host = self.out.text_span(host);
        let path = self.out.text_span(&path);
        self.out.push(VtEvent::Cwd { host, path });
    }

    /// OSC 8.
    fn osc_hyperlink(&mut self, params: &OscParams<'_>) {
        if params.len() <= 2 {
            self.unhandled();
            return;
        }
        let link_params = params.get(1).unwrap_or_default();
        self.join_lossy(params, 2);
        let link = if self.state.scratch.is_empty() {
            None
        } else {
            let id = link_id(link_params);
            let state = &mut *self.state;
            state.interner.hyperlinks.intern(id, &state.scratch)
        };
        self.set_pen_hyperlink(link);
    }

    /// Sets (or clears) the hyperlink of the pen, keeping its image.
    fn set_pen_hyperlink(&mut self, link: Option<HyperlinkId>) {
        let state = &mut *self.state;
        let template = state.grid.screen().cursor().template();
        let current = *state.interner.resolve_extras(template.extras_id());
        let extras = state.interner.extras(Extras {
            hyperlink: link,
            ..current
        });
        state
            .grid
            .screen_mut()
            .set_template(template.with_extras(extras), &mut state.interner);
    }

    /// OSC 9, and OSC 9;4 progress.
    fn osc_notification(&mut self, params: &OscParams<'_>, truncated: bool) {
        if params.len() < 2 {
            self.unhandled();
            return;
        }
        if params.get(1) == Some(b"4") {
            self.osc_progress(params, truncated);
            return;
        }
        if params.len() == 2 && params.get(1).is_some_and(<[u8]>::is_empty) {
            self.unhandled();
            return;
        }
        self.join_lossy(params, 1);
        let title = self.out.text_span("");
        let body = self.out.text_span(&self.state.scratch);
        self.out.push(VtEvent::Notification { title, body });
    }

    fn osc_progress(&mut self, params: &OscParams<'_>, truncated: bool) {
        let number = |index: usize| match params.get(index) {
            None => Some(0),
            Some(bytes) => parse_u32(bytes),
        };
        let (Some(state), Some(percent)) = (number(2), number(3)) else {
            self.unhandled();
            return;
        };
        if truncated {
            self.unhandled();
            return;
        }
        let percent = percent.min(100) as u8;
        let progress = match state {
            0 => Progress::Remove,
            1 => Progress::Set(percent),
            2 => Progress::Error(percent),
            3 => Progress::Indeterminate,
            4 => Progress::Paused(percent),
            _ => {
                self.unhandled();
                return;
            }
        };
        self.out.push(VtEvent::Progress(progress));
    }

    /// OSC 22.
    fn osc_pointer(&mut self, params: &OscParams<'_>) {
        match params.get(1).map(std::str::from_utf8) {
            Some(Ok(name)) if !name.is_empty() => {
                let span = self.out.text_span(name);
                self.out.push(VtEvent::Pointer(span));
            }
            _ => self.unhandled(),
        }
    }

    /// OSC 50.
    fn osc_cursor_shape(&mut self, params: &OscParams<'_>) {
        let shape = match params.get(1) {
            Some(b"CursorShape=0") => CursorShape::Block,
            Some(b"CursorShape=1") => CursorShape::Beam,
            Some(b"CursorShape=2") => CursorShape::Underline,
            _ => {
                self.unhandled();
                return;
            }
        };
        let before = self.state.cursor_style();
        let stored = self.state.stored_cursor_style();
        self.state.cursor_style = Some(CursorStyle { shape, ..stored });
        self.note_cursor_style(before);
    }

    /// OSC 52.
    fn osc_clipboard(&mut self, params: &OscParams<'_>) {
        let (Some(selection), Some(payload)) = (params.get(1), params.get(2)) else {
            self.unhandled();
            return;
        };
        let selection = match selection.first() {
            None | Some(b'c') => ClipboardKind::Clipboard,
            Some(b'p') => ClipboardKind::Primary,
            Some(b's') => ClipboardKind::Secondary,
            Some(_) => {
                self.unhandled();
                return;
            }
        };
        if payload == b"?" {
            self.out.push(VtEvent::ClipboardLoad { selection });
        } else if let Some(text) = self.out.decode_base64(payload) {
            self.out.push(VtEvent::ClipboardStore { selection, text });
        } else {
            let stats = &mut self.state.stats;
            stats.malformed_sequences = stats.malformed_sequences.saturating_add(1);
        }
    }

    /// OSC 133.
    fn osc_shell_mark(&mut self, params: &OscParams<'_>) {
        let (mark, zone) = match params.get(1) {
            Some(b"A") => (ShellMark::PromptStart, Semantic::Prompt),
            Some(b"B") => (ShellMark::PromptEnd, Semantic::Input),
            Some(b"C") => (ShellMark::OutputStart, Semantic::Output),
            Some(b"D") => {
                let exit_code = params
                    .get(2)
                    .and_then(|code| std::str::from_utf8(code).ok())
                    .and_then(|code| code.parse().ok());
                (ShellMark::OutputEnd { exit_code }, Semantic::None)
            }
            _ => {
                self.unhandled();
                return;
            }
        };
        let state = &mut *self.state;
        let template = state.grid.screen().cursor().template();
        state
            .grid
            .screen_mut()
            .set_template(template.with_semantic(zone), &mut state.interner);
        self.out.push(VtEvent::ShellMark(mark));
    }
}

fn parse_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

/// The `id=` value of an OSC 8 parameter list (`key=value` pairs split on `:`), if non-empty.
fn link_id(params: &[u8]) -> Option<&str> {
    params
        .split(|byte| *byte == b':')
        .find_map(|pair| pair.strip_prefix(b"id="))
        .filter(|id| !id.is_empty())
        .and_then(|id| std::str::from_utf8(id).ok())
}

/// Splits an OSC 7 value into the host and the path.
fn split_cwd(text: &str) -> (&str, String) {
    let Some(rest) = text.strip_prefix("file://") else {
        return ("", text.to_owned());
    };
    let Some((host, path)) = rest.split_once('/') else {
        return (rest, String::new());
    };
    let mut decoded = String::from("/");
    decoded.push_str(&percent_decode(path));
    // `/C:` and `/C:/..` are Windows drive paths: drop the slash that `file:///C:` adds.
    let drive = decoded.as_bytes();
    let is_drive = matches!(drive, [b'/', letter, b':', rest @ ..]
        if letter.is_ascii_alphabetic() && matches!(rest.first(), None | Some(b'/' | b'\\')));
    if is_drive {
        decoded.remove(0);
    }
    (host, decoded)
}

/// Decodes `%XX` escapes; a malformed escape stays as written. Invalid UTF-8 is replaced.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let escaped = (byte == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|pair| {
                let high = char::from(*pair.first()?).to_digit(16)?;
                let low = char::from(*pair.get(1)?).to_digit(16)?;
                u8::try_from(high * 16 + low).ok()
            });
        match escaped {
            Some(value) => {
                out.push(value);
                index += 3;
            }
            None => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
