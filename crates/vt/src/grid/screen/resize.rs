//! Row resize: growing pulls history in, shrinking pushes rows into history or drops blank
//! rows below the cursor, and the cursor row always stays on screen.
//!
//! Ids are kept. The only ids that go away are the ones released at the bottom, and they come
//! back only as freshly stamped rows (`newest_issued`).

use crate::cell::Cell;
use crate::grid::ScrollRegion;
use crate::grid::anchor::Anchors;
use crate::grid::screen::Screen;
use crate::grid::{RowId, row::RowRef};

impl Screen {
    /// Resizes the screen to `rows` (cursor, saved cursor and ids follow the rules above) and
    /// resets the scroll region. Returns how many rows left the oldest end of the ring.
    pub(crate) fn resize_rows(&mut self, rows: u16, anchors: &mut Anchors) -> u32 {
        let trimmed = if rows > self.rows {
            self.grow_rows(rows - self.rows);
            0
        } else if rows < self.rows {
            self.shrink_rows(self.rows - rows, anchors)
        } else {
            0
        };
        self.region = ScrollRegion::full(self.rows);
        self.settle();
        trimmed
    }

    /// Adds `k` rows: history is pulled in first (its ids are kept), blank rows fill the rest.
    fn grow_rows(&mut self, k: u16) {
        let history = u16::try_from(self.history_len()).unwrap_or(u16::MAX);
        let pull = history.min(k);
        let appended = k - pull;
        self.rows += pull + appended;
        for _ in 0..appended {
            // Nothing was erased, so the rows are plain blanks, not the pen's erase cell.
            self.push_row(Cell::EMPTY);
        }
    }

    /// Removes `k < rows` rows: blank rows below the cursor are released first, the rest of
    /// the top goes to history. Returns how many history rows the scrollback limit trimmed.
    fn shrink_rows(&mut self, k: u16, anchors: &mut Anchors) -> u32 {
        let from_blank = k.min(self.blank_rows_below_cursor());
        let push = self.cursor_row_index().min(k - from_blank);
        self.release_bottom(u64::from(k - push), anchors);
        self.rows -= k;
        let trimmed = self.trim_history();
        if trimmed > 0 {
            anchors.trim(self.kind.origin(), self.oldest);
        }
        trimmed
    }

    /// Shifts the screen so the cursor lands on screen index `target`, as the host would have
    /// moved it. Returns how many history rows the scrollback limit trimmed.
    pub(crate) fn shift_cursor_to(&mut self, target: u16, anchors: &mut Anchors) -> u32 {
        let index = self.cursor_row_index();
        let mut trimmed = 0u32;
        if index > target {
            // The screen scrolls up: blank rows open at the bottom, the top rows enter history.
            for _ in target..index {
                trimmed = trimmed.saturating_add(self.push_row(Cell::EMPTY));
            }
            if trimmed > 0 {
                anchors.trim(self.kind.origin(), self.oldest);
            }
        } else if index < target {
            // History comes back in at the top and the bottom rows go, like the host cut them.
            let below = self.newest.distance(self.cursor.pos.row);
            let pull = u64::from(target - index)
                .min(u64::from(self.history_len()))
                .min(below);
            self.release_bottom(pull, anchors);
        }
        self.settle();
        trimmed
    }

    /// Sets the viewport offset after a resize: 0 when it followed output, else the distance
    /// from the screen top to `view_top`.
    pub(crate) fn restate_viewport(&mut self, follows_output: bool, view_top: RowId) {
        self.offset = if follows_output {
            0
        } else {
            let distance = self.screen_top().distance(view_top);
            u32::try_from(distance).unwrap_or(u32::MAX)
        };
        self.settle();
    }

    /// Releases the `n` newest ids: their rows go, content anchors on them die, and the new
    /// bottom row no longer continues. The row count is the caller's to keep right.
    fn release_bottom(&mut self, n: u64, anchors: &mut Anchors) {
        // Never release the oldest id: a screen keeps at least one live row.
        let n = n.min(self.newest.distance(self.oldest));
        if n == 0 {
            return;
        }
        let released = self.newest - (n - 1)..self.newest + 1;
        anchors.shift_region(released.clone(), 0, released.clone());
        let mut id = released.start;
        while id < released.end {
            let index = self.slot_index(id);
            if let Some(row) = self.slots.get_mut(index).and_then(Option::take) {
                self.spare = Some(row);
            }
            id = id + 1;
        }
        self.newest = self.newest - n;
        if self.row(self.newest).wrapped()
            && let Some(mut row) = self.row_mut(self.newest)
        {
            row.set_wrapped(false);
        }
    }

    /// Rows at the bottom, below the cursor row, that hold nothing.
    fn blank_rows_below_cursor(&self) -> u16 {
        let mut blank = 0u16;
        let mut id = self.newest;
        while id > self.cursor.pos.row && Self::is_blank(self.row(id)) {
            blank = blank.saturating_add(1);
            id = id - 1;
        }
        blank
    }

    /// A row never written, or written and reset to plain blanks: O(1).
    fn is_blank(row: RowRef<'_>) -> bool {
        !row.is_allocated() || (row.occ() == 0 && row.cells().last() == Some(&Cell::EMPTY))
    }

    /// Brings the viewport and both cursors back inside the screen, then runs the tier-1 check.
    pub(super) fn settle(&mut self) {
        self.offset = self.offset.min(self.history_len());
        let (top, newest) = (self.screen_top(), self.newest);
        for cursor in [&mut self.cursor, &mut self.saved_cursor] {
            cursor.pos.row = cursor.pos.row.clamp(top, newest);
        }
        self.debug_check();
    }
}
