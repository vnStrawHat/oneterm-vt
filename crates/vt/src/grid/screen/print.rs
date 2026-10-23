//! The print path: glyphs, line wrap and tabs.
//!
//! The cursor's pen (`template`) is what a glyph inherits. The caller measures the glyph
//! (`scalar_width`, or `cluster_width` under mode `?2027`) and passes the width in.

use crate::cell::{Cell, CellContent, CellWidth};
use crate::grid::anchor::Anchors;
use crate::grid::row::blank_of;
use crate::grid::screen::{Screen, ScrollReport};
use crate::grid::{Pos, RowId};
use crate::intern::Interner;

/// The modes the print path reads. The grid holds no mode state: the caller passes them in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PrintMode {
    /// Insert mode (IRM): printing shifts the rest of the row right.
    pub insert: bool,
    /// Auto-wrap mode (DECAWM): printing past the last column wraps to the next row.
    pub autowrap: bool,
    /// Reverse-wrap mode (`?45`): backspace at column 0 moves to the last column of a row
    /// that wrapped into this one.
    pub reverse_wrap: bool,
}

impl Default for PrintMode {
    fn default() -> Self {
        PrintMode {
            insert: false,
            autowrap: true,
            reverse_wrap: false,
        }
    }
}

/// Whether `cell` is half of a wide glyph.
fn is_wide_half(cell: Cell) -> bool {
    matches!(cell.width(), CellWidth::Wide | CellWidth::WideSpacer)
}

impl Screen {
    /// Prints `c`, `width` columns wide (0 attaches it to the previous glyph).
    ///
    /// Control characters are ignored. A wide glyph that cannot be placed is dropped and
    /// counted by [`dropped_wide`](Self::dropped_wide). Returns the report of the scroll a
    /// wrap ran, `Some` exactly when one did.
    pub(crate) fn print(
        &mut self,
        c: char,
        width: u8,
        mode: PrintMode,
        interner: &mut Interner,
        anchors: &mut Anchors,
    ) -> Option<ScrollReport> {
        let report = self.print_glyph(c, width, mode, interner, anchors);
        self.debug_check();
        report
    }

    /// Prints the leading run of `text` whose scalars `narrow` maps to a single-column glyph,
    /// under one row borrow. Returns the bytes consumed.
    ///
    /// It stops before anything the general path must decide: insert mode, a pending wrap that
    /// autowrap performs, a scalar `narrow` rejects, a wide-pair half at column 0 or 1 (the
    /// filler above may need releasing), and after filling the last column.
    pub(crate) fn print_narrow_run(
        &mut self,
        text: &str,
        mode: PrintMode,
        narrow: impl Fn(char) -> Option<char>,
    ) -> usize {
        if mode.insert || (self.cursor.pending_wrap && mode.autowrap) {
            return 0;
        }
        if text.chars().next().is_none_or(|c| narrow(c).is_none()) {
            return 0;
        }
        let Pos { row, mut col } = self.cursor.pos;
        if col <= 1 && is_wide_half(self.row(row).cell(col)) {
            return 0;
        }
        let (pen, cols) = (self.cursor.template, self.cols);
        let mut consumed = 0;
        let mut pending_wrap = self.cursor.pending_wrap;
        let Some(mut target) = self.row_mut(row) else {
            return 0;
        };
        for c in text.chars() {
            let Some(mapped) = narrow(c) else { break };
            if col <= 1
                && target
                    .cells()
                    .get(usize::from(col))
                    .is_some_and(|&cell| is_wide_half(cell))
            {
                break;
            }
            target.set(
                col,
                pen.with_content(CellContent::Scalar(mapped))
                    .with_width(CellWidth::Narrow),
            );
            consumed += c.len_utf8();
            if col + 1 < cols {
                col += 1;
            } else {
                pending_wrap = true;
                break;
            }
        }
        self.cursor.pos.col = col;
        self.cursor.pending_wrap = pending_wrap;
        self.debug_check();
        consumed
    }

    fn print_glyph(
        &mut self,
        c: char,
        width: u8,
        mode: PrintMode,
        interner: &mut Interner,
        anchors: &mut Anchors,
    ) -> Option<ScrollReport> {
        if c.is_control() {
            return None;
        }
        if width == 0 {
            self.attach(c, interner);
            return None;
        }
        let mut report = None;
        if self.cursor.pending_wrap && mode.autowrap {
            report = self.wrapline(anchors);
        }
        let span: u16 = if width >= 2 { 2 } else { 1 };
        if mode.insert && self.cursor.pos.col.saturating_add(span) < self.cols {
            let Pos { row, col } = self.cursor.pos;
            // The shift moves or splits a wide pair, so the cell it leaves behind is no longer
            // the one that justified a filler on the row above.
            self.release_filler_if_wide_half(row, col);
            if let Some(mut row) = self.row_mut(row) {
                row.shift_right(col, span);
            }
        }
        let pen = self.cursor.template;
        if span == 1 {
            self.write_cell(
                pen.with_content(CellContent::Scalar(c))
                    .with_width(CellWidth::Narrow),
            );
            self.advance();
            return report;
        }
        if self.cols < 2 {
            self.drop_wide();
            return report;
        }
        if self.cursor.pos.col + 1 >= self.cols {
            if !mode.autowrap {
                self.cursor.pending_wrap = true;
                self.drop_wide();
                return report;
            }
            let filler = pen
                .with_content(CellContent::Scalar(' '))
                .with_width(CellWidth::LeadingWideSpacer);
            self.write_cell(filler);
            report = self.wrapline(anchors);
        }
        self.write_cell(
            pen.with_content(CellContent::Scalar(c))
                .with_width(CellWidth::Wide),
        );
        self.advance();
        self.advance();
        report
    }

    fn drop_wide(&mut self) {
        self.dropped_wide = self.dropped_wide.saturating_add(1);
    }

    /// Moves right one column; at the last column arms the wrap instead, whatever DECAWM says,
    /// so turning autowrap back on over an armed flag still breaks the line.
    fn advance(&mut self) {
        if self.cursor.pos.col + 1 < self.cols {
            self.cursor.pos.col += 1;
        } else {
            self.cursor.pending_wrap = true;
        }
    }

    /// Writes a cell at the cursor and repairs the row above when it held the filler of a
    /// wide glyph this write has just destroyed.
    fn write_cell(&mut self, cell: Cell) {
        let Pos { row, col } = self.cursor.pos;
        // A wide glyph written at column 0 owns the filler above (it just wrapped), so that
        // filler stays whatever it overwrites.
        let owns_filler = col == 0 && cell.width() == CellWidth::Wide;
        if !owns_filler {
            self.release_filler_if_wide_half(row, col);
        }
        if let Some(mut target) = self.row_mut(row) {
            target.set(col, cell);
        }
    }

    /// Releases the filler on the row above when the cell at `(row, col)` is a wide half that
    /// is about to be destroyed or moved.
    fn release_filler_if_wide_half(&mut self, row: RowId, col: u16) {
        if col <= 1 && row > self.oldest && is_wide_half(self.row(row).cell(col)) {
            self.release_leading_filler(row - 1);
        }
    }

    fn release_leading_filler(&mut self, id: RowId) {
        let last_col = self.cols - 1;
        let last = self.row(id).cell(last_col);
        if last.width() != CellWidth::LeadingWideSpacer {
            return;
        }
        if let Some(mut row) = self.row_mut(id) {
            row.repair(last_col, blank_of(last));
        }
    }

    /// Joins a zero-width `c` onto the glyph before the cursor.
    fn attach(&mut self, c: char, interner: &mut Interner) {
        let Pos { row, col } = self.cursor.pos;
        let mut target = if self.cursor.pending_wrap {
            col
        } else {
            col.saturating_sub(1)
        };
        let mut cell = self.row(row).cell(target);
        if cell.width() == CellWidth::WideSpacer && target > 0 {
            target -= 1;
            cell = self.row(row).cell(target);
        }
        let mut cluster = String::new();
        match cell.content() {
            CellContent::Scalar(base) => cluster.push(base),
            CellContent::Grapheme(id) => cluster.push_str(interner.resolve_grapheme(id)),
        }
        cluster.push(c);
        let joined = cell.with_content(CellContent::Grapheme(interner.grapheme(&cluster)));
        if let Some(mut target_row) = self.row_mut(row) {
            target_row.set(target, joined);
        }
        self.debug_check();
    }

    /// Wraps to the start of the next row, marking the current row as continued. Callers gate
    /// it on DECAWM. Returns the report of the scroll it ran, `Some` exactly when one did.
    pub fn wrapline(&mut self, anchors: &mut Anchors) -> Option<ScrollReport> {
        let id = self.cursor.pos.row;
        if let Some(mut row) = self.row_mut(id) {
            row.set_wrapped(true);
        }
        let report = if self.cursor_row_index() + 1 >= self.region.bottom {
            self.linefeed(anchors)
        } else {
            self.cursor.pos.row = self.cursor.pos.row + 1;
            None
        };
        self.cursor.pos.col = 0;
        self.cursor.pending_wrap = false;
        self.sync_anchors(anchors);
        self.debug_check();
        report
    }

    /// `HT` `count` times. Consumed by a pending wrap, which it performs when autowrap is on;
    /// the result is the report of the scroll that wrap ran, `Some` exactly when one did.
    ///
    /// A blank cell the cursor leaves is marked with a tab so text extraction can tell a tab
    /// from spaces.
    pub(crate) fn put_tab(
        &mut self,
        count: u16,
        mode: PrintMode,
        anchors: &mut Anchors,
    ) -> Option<ScrollReport> {
        if self.cursor.pending_wrap {
            return if mode.autowrap {
                self.wrapline(anchors)
            } else {
                None
            };
        }
        for _ in 0..count {
            let Pos { row, col } = self.cursor.pos;
            let cell = self.row(row).cell(col);
            if cell.content() == CellContent::Scalar(' ')
                && let Some(mut target) = self.row_mut(row)
            {
                target.repair(col, cell.with_content(CellContent::Scalar('\t')));
            }
            let next = self.tabs.next_after(col);
            if next == col {
                break;
            }
            self.cursor.pos.col = next;
        }
        self.debug_check();
        None
    }

    /// `CHT`: moves to the `n`th next tab stop, or the last column. Pending wrap is kept.
    pub(crate) fn tab_forward(&mut self, n: u16) {
        for _ in 0..n {
            let col = self.cursor.pos.col;
            let next = self.tabs.next_after(col);
            if next == col {
                break;
            }
            self.cursor.pos.col = next;
        }
        self.debug_check();
    }

    /// `CBT`: moves to the `n`th previous tab stop, or column 0. Pending wrap is kept.
    pub(crate) fn tab_backward(&mut self, n: u16) {
        for _ in 0..n {
            let col = self.cursor.pos.col;
            let prev = self.tabs.prev_before(col);
            if prev == col {
                break;
            }
            self.cursor.pos.col = prev;
        }
        self.debug_check();
    }
}
