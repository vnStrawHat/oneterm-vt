//! The canonical dump (spec 0016, verification.md): one implementation of trimming, style runs,
//! cursor clamping and hashing, so adapters only map an engine cell to [`Cell`].

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A cell colour as the dump names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Color {
    /// The terminal default: `"d"`.
    #[default]
    Default,
    /// Palette entry 0-255 (SGR 30-37 and 90-97 are 0-15): `"i<n>"`.
    Indexed(u8),
    /// Direct colour: `"r<rrggbb>"`.
    Rgb(u8, u8, u8),
}

impl Color {
    fn encode(self) -> String {
        match self {
            Color::Default => "d".to_string(),
            Color::Indexed(index) => format!("i{index}"),
            Color::Rgb(r, g, b) => format!("r{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// The attribute subset the dump compares; everything else an engine stores is ignored.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Flags {
    pub bold: bool,
    pub italic: bool,
    /// Any underline style.
    pub underline: bool,
    pub inverse: bool,
}

impl Flags {
    /// Letters in alphabetical order: `b`, `i`, `r`, `u`.
    fn encode(self) -> String {
        [
            (self.bold, 'b'),
            (self.italic, 'i'),
            (self.inverse, 'r'),
            (self.underline, 'u'),
        ]
        .into_iter()
        .filter_map(|(set, letter)| set.then_some(letter))
        .collect()
    }
}

/// One grid column as an engine reports it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cell<'a> {
    /// The base character plus any combining marks the engine stores beside it; empty for a
    /// blank cell (the dump writes a space).
    pub text: &'a str,
    /// The lead cell of a double-width pair.
    pub wide: bool,
    /// The trailing half of a double-width pair: no text, takes the lead's style.
    pub spacer: bool,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
    /// The hyperlink target, if the engine reports one.
    pub uri: Option<&'a str>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Style {
    fg: Color,
    bg: Color,
    flags: Flags,
}

impl Style {
    fn is_default(self) -> bool {
        self == Style::default()
    }
}

/// `[row, col_start, col_end, fg, bg, flags]`, `col_end` exclusive.
pub type AttrRun = (usize, usize, usize, String, String, String);
/// `[row, col_start, col_end, uri]`, `col_end` exclusive.
pub type LinkRun = (usize, usize, usize, String);

/// The canonical state of one checkpoint. Field order is the JSON order.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Dump {
    pub cursor: (usize, usize),
    pub rows: Vec<String>,
    pub history: Vec<String>,
    pub attrs: Vec<AttrRun>,
    pub links: Vec<LinkRun>,
}

impl Dump {
    /// The byte-stable JSON the hash is taken over.
    pub fn canonical_json(&self) -> String {
        // A struct of strings and integers always serializes.
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn sha256(&self) -> String {
        sha256_hex(self.canonical_json().as_bytes())
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Lower-case hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// Collects the rows an adapter walks and applies the dump rules.
pub struct DumpBuilder {
    columns: usize,
    wants_history: bool,
    cursor: (usize, usize),
    rows: Vec<String>,
    history: Vec<String>,
    attrs: Vec<AttrRun>,
    links: Vec<LinkRun>,
}

impl DumpBuilder {
    /// `wants_history`: the final checkpoint only; earlier ones carry an empty history.
    pub fn new(columns: u16, wants_history: bool) -> Self {
        Self {
            columns: usize::from(columns),
            wants_history,
            cursor: (0, 0),
            rows: Vec::new(),
            history: Vec::new(),
            attrs: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Whether `history_row` calls are kept; adapters skip the walk when they are not.
    pub fn wants_history(&self) -> bool {
        self.wants_history
    }

    /// Active screen cursor, 0-based. A column past the last (pending wrap) clamps to it.
    pub fn cursor(&mut self, row: usize, col: usize) {
        self.cursor = (row, col.min(self.columns.saturating_sub(1)));
    }

    /// The next active-screen row, top to bottom: `fill` pushes one [`Cell`] per column.
    pub fn row(&mut self, fill: impl FnOnce(&mut RowCells)) {
        let row = self.rows.len();
        let mut cells = RowCells::new(true);
        fill(&mut cells);
        let (text, attrs, links) = cells.finish();
        self.rows.push(text);
        self.attrs.extend(
            attrs
                .into_iter()
                .map(|(a, b, fg, bg, fl)| (row, a, b, fg, bg, fl)),
        );
        self.links
            .extend(links.into_iter().map(|(a, b, uri)| (row, a, b, uri)));
    }

    /// The next history row, oldest first, directly above the screen's top row.
    pub fn history_row(&mut self, fill: impl FnOnce(&mut RowCells)) {
        if !self.wants_history {
            return;
        }
        let mut cells = RowCells::new(false);
        fill(&mut cells);
        self.history.push(cells.finish().0);
    }

    pub fn finish(self) -> Dump {
        Dump {
            cursor: self.cursor,
            rows: self.rows,
            history: self.history,
            attrs: self.attrs,
            links: self.links,
        }
    }
}

type RawAttr = (usize, usize, String, String, String);
type RawLink = (usize, usize, String);

/// The columns of one row, pushed left to right.
pub struct RowCells {
    track_runs: bool,
    column: usize,
    text: String,
    lead: (Style, Option<String>),
    attr_start: Option<(usize, Style)>,
    link_start: Option<(usize, String)>,
    attrs: Vec<RawAttr>,
    links: Vec<RawLink>,
}

impl RowCells {
    fn new(track_runs: bool) -> Self {
        Self {
            track_runs,
            column: 0,
            text: String::new(),
            lead: (Style::default(), None),
            attr_start: None,
            link_start: None,
            attrs: Vec::new(),
            links: Vec::new(),
        }
    }

    /// The next column. A spacer adds no text and repeats the lead cell's style and link.
    pub fn cell(&mut self, cell: Cell<'_>) {
        if cell.spacer {
            self.advance_runs();
        } else {
            self.text
                .push_str(if cell.text.is_empty() { " " } else { cell.text });
            let style = Style {
                fg: cell.fg,
                bg: cell.bg,
                flags: cell.flags,
            };
            if self.track_runs {
                self.lead = (style, cell.uri.map(str::to_string));
            }
            self.advance_runs();
        }
        self.column += 1;
    }

    /// Extends or closes the runs for the column about to be counted, from `self.lead`.
    fn advance_runs(&mut self) {
        if !self.track_runs {
            return;
        }
        let column = self.column;
        let (style, uri) = (self.lead.0, self.lead.1.clone());
        let same_style = matches!(&self.attr_start, Some((_, open)) if *open == style);
        if !same_style {
            self.close_attr(column);
            self.open_attr(column, style);
        }
        let same_link = match (&self.link_start, &uri) {
            (Some((_, open)), Some(uri)) => open == uri,
            (None, None) => true,
            _ => false,
        };
        if !same_link {
            self.close_link(column);
            if let Some(uri) = uri {
                self.link_start = Some((column, uri));
            }
        }
    }

    fn open_attr(&mut self, column: usize, style: Style) {
        if !style.is_default() {
            self.attr_start = Some((column, style));
        }
    }

    fn close_attr(&mut self, end: usize) {
        if let Some((start, style)) = self.attr_start.take() {
            self.attrs.push((
                start,
                end,
                style.fg.encode(),
                style.bg.encode(),
                style.flags.encode(),
            ));
        }
    }

    fn close_link(&mut self, end: usize) {
        if let Some((start, uri)) = self.link_start.take() {
            self.links.push((start, end, uri));
        }
    }

    fn finish(mut self) -> (String, Vec<RawAttr>, Vec<RawLink>) {
        let end = self.column;
        self.close_attr(end);
        self.close_link(end);
        self.text.truncate(self.text.trim_end_matches(' ').len());
        (self.text, self.attrs, self.links)
    }
}

#[cfg(test)]
#[path = "dump_tests.rs"]
mod dump_tests;
