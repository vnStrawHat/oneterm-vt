use std::collections::BTreeSet;

use super::*;
use crate::cell::CellContent;
use crate::grid::row::SeqNo;
use crate::grid::screen::{PrintMode, ScreenKind};
use crate::intern::Interner;
use crate::width::scalar_width;

/// A primary screen with its anchors, interner and the reflow scratch list.
struct Rig {
    screen: Screen,
    anchors: Anchors,
    interner: Interner,
    scratch: Vec<Option<Row>>,
}

fn rig(rows: u16, cols: u16, scrollback: u32) -> Rig {
    let mut anchors = Anchors::new();
    let screen = Screen::new(
        ScreenKind::Primary,
        Size { rows, cols },
        scrollback,
        &mut anchors,
    );
    Rig {
        screen,
        anchors,
        interner: Interner::default(),
        scratch: Vec::new(),
    }
}

impl Rig {
    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.screen.print(
                c,
                scalar_width(c),
                PrintMode::default(),
                &mut self.interner,
                &mut self.anchors,
            );
        }
    }

    fn newline(&mut self) {
        self.screen.carriage_return();
        self.screen.linefeed(&mut self.anchors);
    }

    fn reflow(&mut self, cols: u16, batch: u64) -> Reflowed {
        self.screen.set_seq(SeqNo(batch));
        let reflowed = self
            .screen
            .reflow_columns(cols, &mut self.anchors, &mut self.scratch);
        self.screen.sync_anchors(&mut self.anchors);
        assert_eq!(self.screen.check_rows(Some(&self.interner)), Ok(()));
        reflowed
    }

    /// Where every row buffer lives, the spare included.
    fn buffers(&self) -> BTreeSet<usize> {
        self.screen
            .slots
            .iter()
            .flatten()
            .chain(self.screen.spare.as_ref())
            .map(|row| row.cells().as_ptr() as usize)
            .collect()
    }

    fn row_text(&self, id: RowId) -> String {
        let mut text = String::new();
        self.screen
            .row_text(id, &self.interner.graphemes, &mut text);
        text.trim_end().to_string()
    }
}

fn narrow(c: char) -> Cell {
    Cell::EMPTY.with_content(CellContent::Scalar(c))
}

fn wide(c: char) -> Cell {
    narrow(c).with_width(CellWidth::Wide)
}

/// A row of `cells` followed by blanks up to `cols`.
fn row_of(cols: u16, cells: &[Cell]) -> Row {
    let mut row = Row::new(RowId(0), cols, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SeqNo(1));
    for (col, &cell) in cells.iter().enumerate() {
        view.set(col as u16, cell);
    }
    row
}

fn spans(row: &Row, length: usize, cols: u16) -> Vec<RowSpan> {
    let row_at = |_: usize| RowRef::allocated(row);
    let mut reader = Reader::new(&row_at, 1);
    let mut spans = Vec::new();
    lay_out(&mut reader, length, cols, |span| spans.push(span));
    spans
}

fn span(start: usize, end: usize, leading_spacer: bool, wrapped: bool) -> RowSpan {
    RowSpan {
        start,
        end,
        leading_spacer,
        wrapped,
    }
}

#[test]
fn fast_path_keeps_row_buffers() {
    let mut rig = rig(4, 20, 6);
    for i in 0..20 {
        rig.type_text(&format!("L{i}"));
        rig.newline();
    }
    assert_eq!(rig.screen.history_len(), 6);
    let before = rig.buffers();
    let reflowed = rig.reflow(10, 2);
    assert_eq!(reflowed.trimmed, 0);
    let after = rig.buffers();
    assert!(after.is_subset(&before), "a row buffer was allocated");
    assert!(rig.scratch.is_empty());
    assert!(rig.scratch.capacity() >= 10);
    assert_eq!(rig.row_text(rig.screen.screen_top() - 1), "L16");

    // And back up: a grow reallocates each buffer, but the scratch list is reused.
    let capacity = rig.scratch.capacity();
    rig.reflow(20, 3);
    assert_eq!(rig.scratch.capacity(), capacity);
}

#[test]
fn full_history_shrink_recycles() {
    let mut rig = rig(4, 20, 50);
    for i in 0..60 {
        rig.type_text(&format!("{i:015}"));
        rig.newline();
    }
    assert_eq!(rig.screen.history_len(), 50);
    let mut seen = rig.buffers();
    // Each of the 54 live rows becomes two rows of 10, so a naive rewrap would build 108
    // buffers; recycling the source rows and the rows the cap drops needs about a quarter.
    for (batch, cols, most) in [(2, 10, 30), (3, 20, 54), (4, 10, 54)] {
        rig.reflow(cols, batch);
        let now = rig.buffers();
        let fresh = now.difference(&seen).count();
        assert!(fresh <= most, "{fresh} new buffers at {cols} columns");
        seen.extend(now);
    }
    assert!(rig.screen.history_len() <= 50);
}

#[test]
fn host_row_ignores_history_and_rows_below() {
    let mut rig = rig(2, 10, 10);
    rig.type_text("abcdefghijklmnopqrstuvwxy");
    // The screen starts at "klmnopqrst" which wrapped from history; the host re-wraps only
    // what is on screen, so the history row does not push the cursor down.
    assert_eq!(rig.screen.history_len(), 1);
    assert_eq!(rig.screen.host_cursor_row(Size { rows: 4, cols: 20 }), 0);
    assert_eq!(rig.screen.host_cursor_row(Size { rows: 4, cols: 5 }), 2);
    // Same width: the cursor's own index, clamped.
    assert_eq!(rig.screen.host_cursor_row(Size { rows: 4, cols: 10 }), 1);
    assert_eq!(rig.screen.host_cursor_row(Size { rows: 1, cols: 10 }), 0);

    // Rows below the cursor are not read.
    let mut rig = self::rig(4, 10, 10);
    rig.type_text("abcdefghijklmn");
    let measured = rig.screen.host_cursor_row(Size { rows: 4, cols: 5 });
    rig.screen.goto(2, 0);
    rig.type_text("0123456789ab");
    rig.screen.goto(1, 4);
    assert_eq!(measured, 2);
    assert_eq!(
        rig.screen.host_cursor_row(Size { rows: 4, cols: 5 }),
        measured
    );
}

#[test]
fn lay_out_spans() {
    // a b W s c at 3 columns: the glyph would straddle the edge, so it moves down.
    let mut row = row_of(6, &[narrow('a'), narrow('b'), wide('W')]);
    RowMut::new(&mut row, SeqNo(1)).set(4, narrow('c'));
    assert_eq!(
        spans(&row, 5, 3),
        [span(0, 2, true, true), span(2, 5, false, false)]
    );
    // At 2 columns the pair fits the second row exactly.
    assert_eq!(
        spans(&row, 5, 2),
        [
            span(0, 2, false, true),
            span(2, 4, false, true),
            span(4, 5, false, false)
        ]
    );
    // At 6 columns it is one row.
    assert_eq!(spans(&row, 5, 6), [span(0, 5, false, false)]);
    // At 1 column every cell is its own row.
    assert_eq!(spans(&row, 5, 1).len(), 5);
    // An empty line is one empty row.
    assert_eq!(spans(&row, 0, 4), [span(0, 0, false, false)]);
    // A line that fills its last row exactly does not open another.
    assert_eq!(
        spans(&row_of(6, &[narrow('a'); 4]), 4, 2),
        [span(0, 2, false, true), span(2, 4, false, false)]
    );
}

#[test]
fn line_length_trims_exact_blanks_only() {
    let row = row_of(8, &[narrow('a'), narrow('b')]);
    let row_at = |_: usize| RowRef::allocated(&row);
    assert_eq!(line_length(&row_at, 1), 2);

    // Blanks written and then erased stay inside `occ` and are still trimmed.
    let mut erased = row_of(8, &[narrow('a'), narrow('b'), narrow('c')]);
    RowMut::new(&mut erased, SeqNo(2)).fill(1..8, Cell::EMPTY);
    let row_at = |_: usize| RowRef::allocated(&erased);
    assert_eq!(line_length(&row_at, 1), 1);

    // A styled blank is content.
    let styled = Cell::EMPTY.with_style(crate::intern::StyleId(1));
    let row = row_of(8, &[narrow('a'), styled, styled]);
    let row_at = |_: usize| RowRef::allocated(&row);
    assert_eq!(line_length(&row_at, 1), 3);

    // A line of only blank rows is empty; a leading spacer is not part of the line.
    let blank = |_: usize| RowRef::blank(RowId(0), 8);
    assert_eq!(line_length(&blank, 3), 0);
    let mut filler = row_of(4, &[narrow('a'), narrow('b'), narrow('c')]);
    RowMut::new(&mut filler, SeqNo(2)).set(3, Cell::EMPTY.with_width(CellWidth::LeadingWideSpacer));
    let row_at = |_: usize| RowRef::allocated(&filler);
    assert_eq!(line_length(&row_at, 1), 3);
}

#[test]
fn trailing_filler_row_is_not_moved() {
    // A row ending in a filler is a leftover of a wrapped glyph; it is laid out, not moved,
    // so no stray blank survives in the middle of the line.
    let mut rig = rig(3, 5, 5);
    rig.type_text("abcd");
    let id = rig.screen.row_of_index(0);
    if let Some(mut row) = rig.screen.row_mut(id) {
        row.set(4, Cell::EMPTY.with_width(CellWidth::LeadingWideSpacer));
    }
    rig.reflow(8, 2);
    let first = rig.screen.row(rig.screen.oldest());
    assert!(
        first
            .cells()
            .iter()
            .all(|cell| cell.width() == CellWidth::Narrow)
    );
    assert_eq!(rig.row_text(rig.screen.oldest()), "abcd");
}

#[test]
fn reflow_stamps_every_row_with_the_batch() {
    let mut rig = rig(3, 8, 5);
    rig.type_text("abcdefghijkl");
    rig.newline();
    rig.type_text("xyz");
    rig.reflow(5, 7);
    let mut id = rig.screen.oldest();
    while id <= rig.screen.newest() {
        let row = rig.screen.row(id);
        if row.is_allocated() {
            assert_eq!(row.seq(), SeqNo(7));
            assert!(row.flags().contains(crate::grid::row::RowFlags::DIRTY));
        }
        id = id + 1;
    }
}
