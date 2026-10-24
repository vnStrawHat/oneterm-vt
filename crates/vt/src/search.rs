//! Scrollback search over a copy of the grid.
//!
//! Search is two phases. [`GridText::from_terminal`] copies the active screen's text (history and
//! screen) while you hold your lock on the [`Terminal`]; [`search_grid_text`] matches the copy
//! without it, on any thread. A [`SearchMatch`] names [`RowId`]s, so it stays correct while
//! output scrolls underneath it.
//!
//! The copy is one UTF-8 string of logical lines: rows joined by a soft wrap (the `WRAPPED`
//! flag) run together, every other row ends its line with `'\n'`, and an unwrapped row stops at
//! its last non-blank cell. A match never crosses a hard line end. Wide-glyph spacers emit
//! nothing and a grapheme cluster is emitted whole, so a private column table maps a byte
//! offset in the copy back to the cell it came from.
//!
//! Cost: `from_terminal` is one pass over every live row and holds about one byte per ASCII
//! cell up to each row's content end (more for clusters, at most 64 bytes per cell); a search
//! allocates only the result, which stops at [`MAX_MATCHES`].

use crate::cell::{CellContent, CellWidth};
use crate::grid::{RowId, RowRef};
use crate::intern::GraphemeArena;
use crate::terminal::Terminal;

mod literal;
#[cfg(feature = "regex")]
mod regex;
#[cfg(test)]
mod search_tests;

/// The most matches [`search_grid_text`] returns. A result of exactly this length means there
/// may be more.
///
/// The bound keeps a one-letter needle over a screen of one letter from costing six times the
/// copy in matches.
pub const MAX_MATCHES: usize = 100_000;

/// How a literal needle is compared. Both flags are off by default.
///
/// Ignored by `SearchPattern::Regex` (feature `regex`) in every build: write `(?i)` and `\b` in the pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct SearchOptions {
    /// Compare exactly. When `false` (the default) each char is folded to lower case first:
    /// ASCII letters, and any char whose lower-case form is a single non-ASCII char (Latin-1,
    /// Latin Extended, Greek, Cyrillic, Armenian). This is not Unicode case folding: `ß` vs `ss`,
    /// `ς`/`σ`, `ſ`/`s`, `İ`/`i` and the Kelvin sign vs `k` stay distinct.
    /// Use `(?i)` under the `regex` feature for those.
    pub case_sensitive: bool,
    /// Accept a hit only when neither its start nor its end sits inside a word: at each edge,
    /// at least one of the two neighbouring chars (or the text edge) must not be a word char.
    /// A word char is alphanumeric (Unicode) or `_`; a line end is never a word char.
    pub whole_word: bool,
}

/// What to look for.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum SearchPattern<'a> {
    /// A literal string, compared per [`SearchOptions`]. An empty needle, or one that contains
    /// `'\n'`, finds nothing.
    Literal(&'a str),
    /// A compiled regular expression. Available with the `regex` feature.
    ///
    /// Compilation, flags and `RegexBuilder::size_limit` are yours: the crate takes a finished
    /// `Regex`, so a hostile pattern never becomes this crate's problem. The pattern runs over
    /// one logical line at a time (wrapped rows joined): `^` and `$` anchor to the line's first
    /// char and its content end (trailing blanks of the last row are not text); a wide glyph is
    /// one char, a cluster is all its scalars, and `\n` never matches. [`SearchOptions`] is
    /// ignored. An empty-matching pattern yields one empty match per char boundary of each
    /// line, the end included; on a blank row that is one match at column 0.
    ///
    /// `(?i)` is Unicode simple case folding, wider than the literal matcher's lower-casing,
    /// so an escaped needle agrees with the literal matcher on ASCII, Latin-1 and Cyrillic text
    /// but not on `ſ`, `ς`, the Kelvin sign or `ß`.
    #[cfg(feature = "regex")]
    Regex(&'a ::regex::Regex),
}

/// One match: where it starts and where it ends (exclusive).
///
/// A match ending on a row's last cell has `end_col == cols`. An empty regex match has
/// `end_row == row` and `end_col == start_col`; `start_col` may equal the row's cell count
/// (an empty match at the line end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    /// The row holding the first cell.
    pub row: RowId,
    /// The column of the first cell.
    pub start_col: usize,
    /// The row holding the end; differs from `row` when the match spans a wrap.
    pub end_row: RowId,
    /// The exclusive end column on `end_row`.
    pub end_col: usize,
}

impl SearchMatch {
    /// The start row as a line relative to the top screen row `screen_top`
    /// ([`Screen::screen_top`](crate::grid::Screen::screen_top)): negative is history, `-1`
    /// the newest history row. Clamped to the `i32` range.
    pub fn grid_line(&self, screen_top: RowId) -> i32 {
        if self.row >= screen_top {
            i32::try_from(self.row.distance(screen_top)).unwrap_or(i32::MAX)
        } else {
            i32::try_from(screen_top.distance(self.row)).map_or(i32::MIN, |lines| -lines)
        }
    }

    /// The start row as a viewport row, given the top screen row and the screen's
    /// [`scroll_offset`](crate::grid::Screen::scroll_offset). Saturates; filtering out what is
    /// not in view is yours.
    pub fn display_row(&self, screen_top: RowId, display_offset: usize) -> i32 {
        self.grid_line(screen_top)
            .saturating_add(i32::try_from(display_offset).unwrap_or(i32::MAX))
    }
}

/// A copy of the active screen's text, safe to search without any lock.
///
/// It is the grid as it stood. Matches stay valid while their rows are live:
/// [`VtEvent::RowsTrimmed`](crate::VtEvent::RowsTrimmed) ends rows older than `oldest`, and a
/// column resize that reflows gives primary rows fresh ids, so re-copy and search again.
/// `GridText` owns plain data and is `Send + Sync`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridText {
    oldest: RowId,
    text: String,
    /// One entry per live row plus an end sentinel whose `text_start` is `text.len()`.
    rows: Vec<RowText>,
    /// The column tables of the `ColumnMap::Table` rows, back to back.
    columns: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RowText {
    /// Byte offset of the row's first char in `GridText::text`.
    text_start: usize,
    map: ColumnMap,
}

/// How a byte offset inside a row's text becomes a column.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ColumnMap {
    /// Every cell emitted exactly one byte: the column is the byte offset.
    Identity,
    /// `columns[start..end]`: one entry per cell (the byte offset of its text, relative to the
    /// row; a spacer repeats the previous cell's) plus an end sentinel holding the text length.
    Table { start: usize, end: usize },
}

/// Which side of a match a byte offset is.
#[derive(Debug, Clone, Copy)]
enum Edge {
    Start,
    End,
}

impl GridText {
    /// Copies the active screen's text, history included, in one pass.
    ///
    /// Call it under your lock. Memory is about one byte per ASCII cell up to each row's
    /// content end, plus 32 bytes per row (100 000 rows of 200 columns, half full, is about
    /// 13 MB); rows with wide glyphs or clusters add 4 bytes per cell of column table. There is
    /// no cap: the history length is your `scrollback_limit`.
    pub fn from_terminal(term: &Terminal) -> GridText {
        let screen = term.screen();
        let graphemes = &term.interner().graphemes;
        let live = screen.row_range();
        let count = usize::try_from(live.end.distance(live.start)).unwrap_or(0);
        let mut copy = GridText {
            oldest: live.start,
            text: String::new(),
            rows: Vec::with_capacity(count.saturating_add(1)),
            columns: Vec::new(),
        };
        let mut id = live.start;
        while id < live.end {
            let is_newest = id + 1 >= live.end;
            copy.push_row(screen.row(id), graphemes, is_newest);
            id = id + 1;
        }
        copy.rows.push(RowText {
            text_start: copy.text.len(),
            map: ColumnMap::Identity,
        });
        copy
    }

    /// The oldest live row id at copy time.
    pub fn oldest(&self) -> RowId {
        self.oldest
    }

    /// How many live rows were copied.
    pub fn rows(&self) -> usize {
        self.rows.len().saturating_sub(1)
    }

    fn push_row(&mut self, row: RowRef<'_>, graphemes: &GraphemeArena, is_newest: bool) {
        let text_start = self.text.len();
        let column_start = self.columns.len();
        let mut count = usize::from(row.text_end());
        // `text_end` counts spacers as blank, so it stops before the spacer of a trailing wide
        // glyph; the spacer is in the table so that a match ends at the pair's end.
        if row
            .cells()
            .get(count)
            .is_some_and(|cell| cell.width() == CellWidth::WideSpacer)
        {
            count += 1;
        }
        let mut one_byte_per_cell = true;
        let mut previous_entry = 0u32;
        for cell in row.cells().iter().take(count) {
            if cell.width().is_spacer() {
                one_byte_per_cell = false;
                self.columns.push(previous_entry);
                continue;
            }
            let before = self.text.len();
            previous_entry = offset_in_row(before - text_start);
            self.columns.push(previous_entry);
            match cell.content() {
                CellContent::Scalar(c) => self.text.push(c),
                CellContent::Grapheme(id) => match graphemes.resolve(id) {
                    "" => self.text.push(' '),
                    cluster => self.text.push_str(cluster),
                },
            }
            one_byte_per_cell &= self.text.len() - before == 1;
        }
        let map = if one_byte_per_cell {
            self.columns.truncate(column_start);
            ColumnMap::Identity
        } else {
            self.columns
                .push(offset_in_row(self.text.len() - text_start));
            ColumnMap::Table {
                start: column_start,
                end: self.columns.len(),
            }
        };
        self.rows.push(RowText { text_start, map });
        // A soft wrap joins the next row; the newest row always ends its line.
        if !row.wrapped() || is_newest {
            self.text.push('\n');
        }
    }

    /// The match covering text bytes `start..end`; `None` only if the tables disagree with
    /// the text, which they cannot.
    fn span(&self, start: usize, end: usize) -> Option<SearchMatch> {
        let (row, start_col) = self.locate(start, Edge::Start)?;
        let (end_row, end_col) = if end <= start {
            (row, start_col)
        } else {
            self.locate(end, Edge::End)?
        };
        Some(SearchMatch {
            row,
            start_col,
            end_row,
            end_col,
        })
    }

    /// The row and column of byte offset `at`: a start rounds down into its cell, an end
    /// rounds up past it.
    fn locate(&self, at: usize, edge: Edge) -> Option<(RowId, usize)> {
        let index = match edge {
            Edge::Start => self.rows.partition_point(|row| row.text_start <= at),
            Edge::End => self.rows.partition_point(|row| row.text_start < at),
        }
        .checked_sub(1)?;
        let row = self.rows.get(index)?;
        let relative = at.saturating_sub(row.text_start);
        let id = self.oldest + u64::try_from(index).unwrap_or(u64::MAX);
        Some((id, self.column(&row.map, relative, edge)))
    }

    fn column(&self, map: &ColumnMap, relative: usize, edge: Edge) -> usize {
        let ColumnMap::Table { start, end } = *map else {
            return relative;
        };
        let table = self.columns.get(start..end).unwrap_or(&[]);
        let relative = offset_in_row(relative);
        match edge {
            Edge::Start => {
                // The cell whose text holds `relative`; a glyph wins over its spacer.
                let containing = table.partition_point(|&entry| entry <= relative);
                let first_byte = containing
                    .checked_sub(1)
                    .and_then(|last| table.get(last))
                    .copied()
                    .unwrap_or(0);
                table.partition_point(|&entry| entry < first_byte)
            }
            Edge::End => table.partition_point(|&entry| entry < relative),
        }
    }
}

/// A byte offset inside one row's text, which is at most `2048 * 64` bytes.
fn offset_in_row(bytes: usize) -> u32 {
    u32::try_from(bytes).unwrap_or(u32::MAX)
}

/// Finds every match of `pattern` in `text`, top to bottom and left to right, without
/// overlap. At most [`MAX_MATCHES`] are returned.
pub fn search_grid_text(
    text: &GridText,
    pattern: SearchPattern<'_>,
    options: SearchOptions,
) -> Vec<SearchMatch> {
    match pattern {
        SearchPattern::Literal(needle) => literal::search(text, needle, options),
        #[cfg(feature = "regex")]
        SearchPattern::Regex(regex) => regex::search(text, regex),
    }
}
