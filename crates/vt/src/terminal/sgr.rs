//! SGR: turning `CSI Pm m` into the pen's style, and the style back into SGR text for DECRQSS.

use std::fmt::Write as _;

use crate::cell::{Attrs, Color, NamedColor, Rgb, Style};
use crate::parser::ParamGroups;

use super::csi::Args;
use super::dispatch::Handler;

/// The sixteen named colours in palette order.
const NAMED: [NamedColor; 16] = [
    NamedColor::Black,
    NamedColor::Red,
    NamedColor::Green,
    NamedColor::Yellow,
    NamedColor::Blue,
    NamedColor::Magenta,
    NamedColor::Cyan,
    NamedColor::White,
    NamedColor::BrightBlack,
    NamedColor::BrightRed,
    NamedColor::BrightGreen,
    NamedColor::BrightYellow,
    NamedColor::BrightBlue,
    NamedColor::BrightMagenta,
    NamedColor::BrightCyan,
    NamedColor::BrightWhite,
];

/// Which colour of the style an extended-colour parameter sets.
#[derive(Clone, Copy)]
enum Target {
    Foreground,
    Background,
    Underline,
}

impl Handler<'_> {
    /// `CSI Pm m`: edits the pen's style group by group. A group the engine does not know is
    /// skipped and the rest still applies.
    pub(super) fn sgr(&mut self, args: &mut Args<'_>) {
        let template = self.state.grid.screen().cursor().template();
        let mut style = *self.state.interner.resolve_style(template.style_id());
        let mut groups = args.groups();
        while let Some(group) = groups.next() {
            apply_group(&mut style, group, &mut groups);
        }
        let id = self.state.interner.style(style);
        if id == template.style_id() {
            return;
        }
        let state = &mut *self.state;
        state
            .grid
            .screen_mut()
            .set_template(template.with_style(id), &mut state.interner);
    }
}

fn set_underline(style: &mut Style, underline: Attrs) {
    style.attrs.remove(Attrs::ALL_UNDERLINES);
    style.attrs.insert(underline);
}

fn apply_group(style: &mut Style, group: &[u16], rest: &mut ParamGroups<'_>) {
    let Some((&code, sub)) = group.split_first() else {
        return;
    };
    match code {
        0 => *style = Style::DEFAULT,
        1 => style.attrs.insert(Attrs::BOLD),
        2 => style.attrs.insert(Attrs::DIM),
        3 => style.attrs.insert(Attrs::ITALIC),
        4 => match sub.first().copied() {
            None | Some(1) => set_underline(style, Attrs::UNDERLINE),
            Some(0) => style.attrs.remove(Attrs::ALL_UNDERLINES),
            Some(2) => set_underline(style, Attrs::DOUBLE_UNDERLINE),
            Some(3) => set_underline(style, Attrs::UNDERCURL),
            Some(4) => set_underline(style, Attrs::DOTTED_UNDERLINE),
            Some(5) => set_underline(style, Attrs::DASHED_UNDERLINE),
            Some(_) => set_underline(style, Attrs::UNDERLINE),
        },
        5 => style.attrs.insert(Attrs::BLINK_SLOW),
        6 => style.attrs.insert(Attrs::BLINK_FAST),
        7 => style.attrs.insert(Attrs::INVERSE),
        8 => style.attrs.insert(Attrs::HIDDEN),
        9 => style.attrs.insert(Attrs::STRIKEOUT),
        21 => set_underline(style, Attrs::DOUBLE_UNDERLINE),
        22 => style.attrs.remove(Attrs::BOLD | Attrs::DIM),
        23 => style.attrs.remove(Attrs::ITALIC),
        24 => style.attrs.remove(Attrs::ALL_UNDERLINES),
        25 => style.attrs.remove(Attrs::BLINK_SLOW | Attrs::BLINK_FAST),
        27 => style.attrs.remove(Attrs::INVERSE),
        28 => style.attrs.remove(Attrs::HIDDEN),
        29 => style.attrs.remove(Attrs::STRIKEOUT),
        30..=37 => style.fg = named(code - 30),
        38 => extended_color(style, Target::Foreground, sub, rest),
        39 => style.fg = Color::Named(NamedColor::Foreground),
        40..=47 => style.bg = named(code - 40),
        48 => extended_color(style, Target::Background, sub, rest),
        49 => style.bg = Color::Named(NamedColor::Background),
        53 => style.attrs.insert(Attrs::OVERLINE),
        55 => style.attrs.remove(Attrs::OVERLINE),
        58 => extended_color(style, Target::Underline, sub, rest),
        59 => style.underline_color = None,
        90..=97 => style.fg = named(code - 90 + 8),
        100..=107 => style.bg = named(code - 100 + 8),
        _ => {}
    }
}

fn named(index: u16) -> Color {
    NAMED
        .get(usize::from(index))
        .map_or(Color::Named(NamedColor::Foreground), |name| {
            Color::Named(*name)
        })
}

/// `38`, `48` or `58`: the colon form carries the colour in `sub`; the semicolon form reads it
/// from the next groups. A missing or out-of-range value drops the colour after what was read.
fn extended_color(style: &mut Style, target: Target, sub: &[u16], rest: &mut ParamGroups<'_>) {
    let color = if sub.is_empty() {
        semicolon_color(rest)
    } else {
        colon_color(sub)
    };
    let Some(color) = color else {
        return;
    };
    match target {
        Target::Foreground => style.fg = color,
        Target::Background => style.bg = color,
        Target::Underline => style.underline_color = Some(color),
    }
}

fn channel(value: Option<u16>) -> Option<u8> {
    value.and_then(|value| u8::try_from(value).ok())
}

/// `5;n` or `2;r;g;b` as separate groups. Stops reading at the first bad value.
fn semicolon_color(rest: &mut ParamGroups<'_>) -> Option<Color> {
    let mut next = || rest.next().and_then(|group| group.first().copied());
    match next()? {
        5 => channel(next()).map(Color::Palette),
        2 => {
            let r = channel(next())?;
            let g = channel(next())?;
            let b = channel(next())?;
            Some(Color::Rgb(Rgb { r, g, b }))
        }
        _ => None,
    }
}

/// `5:n`, `2:r:g:b` or `2:cs:r:g:b` (the colour-space id is skipped).
fn colon_color(sub: &[u16]) -> Option<Color> {
    let (&kind, values) = sub.split_first()?;
    match kind {
        5 => channel(values.first().copied()).map(Color::Palette),
        2 => {
            let values = if values.len() > 3 {
                values.get(1..)?
            } else {
                values
            };
            let r = channel(values.first().copied())?;
            let g = channel(values.get(1).copied())?;
            let b = channel(values.get(2).copied())?;
            Some(Color::Rgb(Rgb { r, g, b }))
        }
        _ => None,
    }
}

/// The SGR parameters (without `CSI` and `m`) that rebuild `style` from the default style, as
/// DECRQSS reports them. Named slots beyond the sixteen colours are the default and omitted.
pub(super) fn sgr_parameters(style: &Style) -> String {
    let mut out = String::from("0");
    let flags: [(Attrs, &str); 8] = [
        (Attrs::BOLD, "1"),
        (Attrs::DIM, "2"),
        (Attrs::ITALIC, "3"),
        (Attrs::BLINK_SLOW, "5"),
        (Attrs::BLINK_FAST, "6"),
        (Attrs::INVERSE, "7"),
        (Attrs::HIDDEN, "8"),
        (Attrs::STRIKEOUT, "9"),
    ];
    for (flag, code) in flags {
        if style.attrs.contains(flag) {
            out.push(';');
            out.push_str(code);
        }
    }
    let underlines: [(Attrs, &str); 5] = [
        (Attrs::UNDERLINE, "4"),
        (Attrs::DOUBLE_UNDERLINE, "4:2"),
        (Attrs::UNDERCURL, "4:3"),
        (Attrs::DOTTED_UNDERLINE, "4:4"),
        (Attrs::DASHED_UNDERLINE, "4:5"),
    ];
    if let Some((_, code)) = underlines
        .iter()
        .find(|(flag, _)| style.attrs.contains(*flag))
    {
        out.push(';');
        out.push_str(code);
    }
    if style.attrs.contains(Attrs::OVERLINE) {
        out.push_str(";53");
    }
    push_color(&mut out, style.fg, 30, 90, 38);
    push_color(&mut out, style.bg, 40, 100, 48);
    if let Some(color) = style.underline_color {
        push_color(&mut out, color, 0, 0, 58);
    }
    out
}

/// Appends one colour: a short code for a named colour (`base` for 0-7, `bright` for 8-15,
/// none when both are 0), else the extended form behind `extended`.
fn push_color(out: &mut String, color: Color, base: u16, bright: u16, extended: u16) {
    // Writing to a `String` cannot fail.
    match color {
        Color::Named(name) => {
            let Some(index) = NAMED.iter().position(|candidate| *candidate == name) else {
                return;
            };
            let index = index as u16;
            if base == 0 {
                return;
            }
            let code = if index < 8 {
                base + index
            } else {
                bright + index - 8
            };
            let _ = write!(out, ";{code}");
        }
        Color::Palette(n) => {
            let _ = write!(out, ";{extended};5;{n}");
        }
        Color::Rgb(Rgb { r, g, b }) => {
            let _ = write!(out, ";{extended};2;{r};{g};{b}");
        }
    }
}
