//! The print path: glyphs, line wrap and tabs.
//!
//! The cursor's pen (`template`) is what a glyph inherits. The caller measures the glyph
//! (`scalar_width`, or `cluster_width` under mode `?2027`) and passes the width in.

use crate::cell::{Cell, CellContent, CellWidth};
use crate::grid::anchor::Anchors;
use crate::grid::row::blank_of;
use crate::grid::screen::{Charset, Screen, ScrollReport};
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

/// Length of the leading printable ASCII (`0x20..=0x7E`) of `bytes`: whole 16-byte blocks
/// with a branch-free test, then bytes. Text from the parser's ASCII path is all printable.
fn printable_ascii_prefix(bytes: &[u8]) -> usize {
    let mut done = 0;
    for block in bytes.chunks_exact(16) {
        let outside = block
            .iter()
            .fold(false, |bad, &byte| bad | (byte.wrapping_sub(0x20) >= 0x5F));
        if outside {
            break;
        }
        done += 16;
    }
    let rest = bytes.get(done..).unwrap_or_default();
    done + rest
        .iter()
        .position(|&byte| byte.wrapping_sub(0x20) >= 0x5F)
        .unwrap_or(rest.len())
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

    /// Prints the leading run of `text` whose scalars `glyph` maps to a one- or two-column
    /// glyph, under one row borrow. Returns the bytes consumed.
    ///
    /// `glyph` gives the mapped scalar and its width, or `None` for a width-0 scalar or a
    /// control. `charset` is the set `glyph` maps through; under [`Charset::Ascii`] printable
    /// ASCII bypasses `glyph` and is written in bulk.
    ///
    /// It stops before anything the general path must decide: insert mode, a pending wrap that
    /// autowrap performs, a scalar `glyph` rejects, a wide glyph that needs the last column
    /// (the filler and the wrap), a wide-pair half at column 0 or 1 (the filler above may need
    /// releasing), and after filling the last column.
    pub(crate) fn print_run(
        &mut self,
        text: &str,
        mode: PrintMode,
        charset: Charset,
        glyph: impl Fn(char) -> Option<(char, u8)>,
    ) -> usize {
        if mode.insert || (self.cursor.pending_wrap && mode.autowrap) {
            return 0;
        }
        if charset == Charset::Ascii {
            let written = self.print_ascii(text.as_bytes());
            if written > 0 {
                return written;
            }
        }
        self.print_glyph_run(text, glyph)
    }

    /// The general part of [`print_run`](Self::print_run): one glyph at a time, writing the run
    /// with one pair release at each edge and one `occ` and hint update.
    ///
    /// Kept out of line: inlined, its frame slowed the ASCII path of short prints.
    #[inline(never)]
    fn print_glyph_run(&mut self, text: &str, glyph: impl Fn(char) -> Option<(char, u8)>) -> usize {
        let Some((_, first_width)) = text.chars().next().and_then(&glyph) else {
            return 0;
        };
        let Pos { row, mut col } = self.cursor.pos;
        if col <= 1 && is_wide_half(self.row(row).cell(col)) {
            return 0;
        }
        // A wide glyph that needs the last column is the general path's (filler and wrap).
        if first_width == 2 && col + 1 >= self.cols {
            return 0;
        }
        let (pen, cols) = (self.cursor.template, self.cols);
        let mut consumed = 0;
        let mut pending_wrap = self.cursor.pending_wrap;
        let Some(mut target) = self.row_mut(row) else {
            return 0;
        };
        let start = col;
        let mut orphan = false;
        target.begin_run(start);
        for c in text.chars() {
            let Some((mapped, width)) = glyph(c) else {
                break;
            };
            if col <= 1
                && target
                    .cells()
                    .get(usize::from(col))
                    .is_some_and(|&cell| is_wide_half(cell))
            {
                break;
            }
            if width == 2 {
                if col + 1 >= cols {
                    break;
                }
                orphan = target.put_glyph(
                    usize::from(col),
                    pen.with_content(CellContent::Scalar(mapped))
                        .with_width(CellWidth::Wide),
                );
                consumed += c.len_utf8();
                if col + 2 < cols {
                    col += 2;
                    continue;
                }
                col += 1;
                pending_wrap = true;
                break;
            }
            orphan = target.put_glyph(
                usize::from(col),
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
        let end = if pending_wrap { cols } else { col };
        if end > start {
            target.finish_run(end, pen, orphan);
        }
        self.cursor.pos.col = col;
        self.cursor.pending_wrap = pending_wrap;
        self.debug_check();
        consumed
    }

    /// Writes the leading printable-ASCII bytes of `bytes` that fit the cursor's row, in one
    /// pass. Returns how many it wrote; 0 leaves everything untouched.
    ///
    /// The scan stops at the row end: a longer scan would re-read the rest of a long run at
    /// every row it wraps through.
    fn print_ascii(&mut self, bytes: &[u8]) -> usize {
        let Pos { row, col } = self.cursor.pos;
        let room = usize::from(self.cols - col);
        let head = bytes.get(..room).unwrap_or(bytes);
        let count = printable_ascii_prefix(head);
        // A wide-pair half at column 0 or 1 may have a filler on the row above that only the
        // per-scalar path releases.
        let first = usize::from(col);
        if count == 0
            || (first..(first + count).min(2)).any(|c| is_wide_half(self.row(row).cell(c as u16)))
        {
            return 0;
        }
        let pen = self.cursor.template;
        let Some(mut target) = self.row_mut(row) else {
            return 0;
        };
        target.write_ascii(col, &head[..count], pen);
        // `count <= room`, so the sum fits the row.
        let end = col + count as u16;
        if end < self.cols {
            self.cursor.pos.col = end;
        } else {
            self.cursor.pos.col = self.cols - 1;
            self.cursor.pending_wrap = true;
        }
        self.debug_check();
        count
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
            self.attach_marks(c.encode_utf8(&mut [0; 4]), interner);
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

    /// Joins the zero-width `marks` onto the glyph before the cursor with one intern. When the
    /// arena refuses the cluster the cell keeps what it showed (the marks are dropped, and the
    /// arena counts it).
    pub(crate) fn attach_marks(&mut self, marks: &str, interner: &mut Interner) {
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
        let id = interner.graphemes.intern_joined(cell.content(), marks);
        if id.0 != 0
            && let Some(mut target_row) = self.row_mut(row)
        {
            target_row.set(target, cell.with_content(CellContent::Grapheme(id)));
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
