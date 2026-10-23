//! Erasing and in-row edits. Everything fills with the cursor's erase cell: a blank that
//! carries only the pen's background.

use crate::cell::Cell;
use crate::grid::ScrollRegion;
use crate::grid::anchor::Anchors;
use crate::grid::row::{RowMut, RowRef};
use crate::grid::screen::{Screen, ScreenKind, ScrollReport};
use crate::intern::Interner;

/// How much of the cursor row `EL` erases.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LineClear {
    /// From the cursor to the end of the row (`EL 0`).
    Right,
    /// From the start of the row to the cursor (`EL 1`).
    Left,
    /// The whole row (`EL 2`).
    All,
}

/// How much of the display `ED` erases.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DisplayClear {
    /// From the cursor to the end of the screen (`ED 0`).
    Below,
    /// From the start of the screen to the cursor (`ED 1`).
    Above,
    /// The whole screen (`ED 2`).
    All,
    /// The scrollback (`ED 3`).
    Saved,
}

impl Screen {
    /// Runs `edit` on the cursor row with the erase cell. A row never written stays unwritten
    /// when the erase cell is the plain blank: there is nothing to erase.
    fn edit_cursor_row(&mut self, edit: impl FnOnce(&mut RowMut<'_>, Cell)) {
        let id = self.cursor.pos.row;
        let erase = self.cursor.erase;
        if erase == Cell::EMPTY && !self.row(id).is_allocated() {
            return;
        }
        if let Some(mut row) = self.row_mut(id) {
            edit(&mut row, erase);
        }
        self.debug_check();
    }

    /// `EL`. `Right` does nothing while a wrap is pending: the cursor is past the last cell.
    pub(crate) fn erase_line(&mut self, mode: LineClear) {
        let col = self.cursor.pos.col;
        let cols = self.cols;
        match mode {
            LineClear::Right if self.cursor.pending_wrap => {}
            LineClear::Right => self.edit_cursor_row(|row, erase| row.fill(col..cols, erase)),
            LineClear::Left => self.edit_cursor_row(|row, erase| row.fill(0..col + 1, erase)),
            LineClear::All => self.edit_cursor_row(|row, erase| row.fill(0..cols, erase)),
        }
    }

    /// `ECH`: erases `n` cells from the cursor.
    pub(crate) fn erase_chars(&mut self, n: u16) {
        if n == 0 {
            return;
        }
        let col = self.cursor.pos.col;
        let end = col.saturating_add(n).min(self.cols);
        self.edit_cursor_row(|row, erase| row.fill(col..end, erase));
    }

    /// `DCH`: deletes `n` cells at the cursor, pulling the rest of the row left.
    pub(crate) fn delete_chars(&mut self, n: u16) {
        if n == 0 {
            return;
        }
        let col = self.cursor.pos.col;
        self.edit_cursor_row(|row, erase| row.delete_cells(col, n, erase));
    }

    /// `ICH`: opens `n` blank cells at the cursor, pushing the rest of the row right.
    pub(crate) fn insert_blanks(&mut self, n: u16) {
        if n == 0 {
            return;
        }
        let col = self.cursor.pos.col;
        self.edit_cursor_row(|row, erase| row.insert_cells(col, n, erase));
    }

    /// `ED`. `All` on the primary screen scrolls the occupied rows into the history instead of
    /// destroying them, and returns what the scroll did; so does `Saved`, when it dropped
    /// anything.
    pub(crate) fn erase_display(
        &mut self,
        mode: DisplayClear,
        anchors: &mut Anchors,
        interner: &Interner,
    ) -> Option<ScrollReport> {
        let index = self.cursor_row_index();
        let col = self.cursor.pos.col;
        let cols = self.cols;
        let report = match mode {
            DisplayClear::Below => {
                self.edit_cursor_row(|row, erase| row.fill(col..cols, erase));
                self.reset_screen_rows(index + 1, self.rows);
                None
            }
            DisplayClear::Above => {
                self.reset_screen_rows(0, index);
                self.edit_cursor_row(|row, erase| row.fill(0..col + 1, erase));
                None
            }
            DisplayClear::All if self.kind == ScreenKind::Alternate => {
                self.reset_screen_rows(0, self.rows);
                None
            }
            DisplayClear::All => {
                let occupied = self.occupied_rows(interner);
                let report = self.scroll_up(ScrollRegion::full(self.rows), occupied, anchors);
                self.reset_screen_rows(0, self.rows - occupied);
                Some(report)
            }
            DisplayClear::Saved => {
                let report = self.clear_history(anchors);
                report.trimmed.map(|_| report)
            }
        };
        self.debug_check();
        report
    }

    /// One more than the index of the last screen row holding something worth keeping.
    fn occupied_rows(&self, interner: &Interner) -> u16 {
        (0..self.rows)
            .rev()
            .find(|&index| has_content(self.row(self.row_of_index(index)), interner))
            .map_or(0, |index| index + 1)
    }
}

fn has_content(row: RowRef<'_>, interner: &Interner) -> bool {
    // Cells at or past `occ` all equal the last cell, so one of them stands for the tail.
    let scanned = (usize::from(row.occ()) + 1).min(row.cells().len());
    row.cells()
        .get(..scanned)
        .unwrap_or(row.cells())
        .iter()
        .any(|cell| !cell.is_erasable(interner))
}
