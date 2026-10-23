//! Tier 1 and tier 2 integrity checks of one screen.
//!
//! Tier 1 is O(1) and runs at the end of every mutating method of a debug build. Tier 2 walks
//! rows; by default only the rows a batch can have touched, with `vt-paranoid` the whole
//! history plus a scan for stale slots.

use crate::grid::integrity::IntegrityViolation as Violation;
use crate::grid::row::check_cell_ids;
use crate::grid::screen::{Cursor, Screen, ScreenKind};
use crate::grid::{RowId, ScrollRegion};
use crate::intern::Interner;

impl Screen {
    /// Checks the O(1) invariants.
    pub(crate) fn check_bounds(&self) -> Result<(), Violation> {
        if self.oldest > self.newest || self.newest > self.newest_issued {
            return Err(Violation::IdsOutOfOrder);
        }
        let span = self.newest.distance(self.oldest);
        if span + 1 < u64::from(self.rows) {
            return Err(Violation::TooFewLiveRows);
        }
        if span >= self.slots.len() as u64 {
            return Err(Violation::RingOverflow);
        }
        if self.history_len() > self.scrollback_limit {
            return Err(Violation::HistoryOverLimit);
        }
        if self.offset > self.history_len() {
            return Err(Violation::OffsetPastHistory);
        }
        let past_lane = self.kind == ScreenKind::Primary && self.newest >= RowId::ALT_ORIGIN;
        if self.oldest < self.kind.origin() || past_lane {
            return Err(Violation::LaneOverlap);
        }
        self.cursor_inside(&self.cursor, false)?;
        self.cursor_inside(&self.saved_cursor, true)?;
        let ScrollRegion { top, bottom } = self.region;
        if top >= bottom || bottom > self.rows {
            return Err(Violation::RegionInvalid);
        }
        Ok(())
    }

    fn cursor_inside(&self, cursor: &Cursor, saved: bool) -> Result<(), Violation> {
        let on_screen = cursor.pos.row >= self.screen_top() && cursor.pos.row <= self.newest;
        if on_screen && cursor.pos.col < self.cols {
            Ok(())
        } else {
            Err(Violation::CursorOutside { saved })
        }
    }

    /// The first id the row walk visits.
    ///
    /// Bounded: one row above the batch's first screen row or the viewport, whichever is
    /// older. A batch only writes rows from `batch_lo` on, and the extra row covers the
    /// cross-row spacer release. With `vt-paranoid`: the whole history.
    pub(crate) fn walk_start(&self) -> RowId {
        if cfg!(feature = "vt-paranoid") {
            return self.oldest;
        }
        let start = self.batch_lo.min(self.visible_top()) - 1;
        start.max(self.oldest).min(self.newest)
    }

    /// Checks the rows from [`walk_start`](Self::walk_start) on, and the pens against `interner`.
    pub(crate) fn check_rows(&self, interner: Option<&Interner>) -> Result<(), Violation> {
        self.check_bounds()?;
        let start = self.walk_start();
        for step in 0..=self.newest.distance(start) {
            let id = start + step;
            let Some(row) = self.slots.get(self.slot_index(id)).and_then(Option::as_ref) else {
                continue;
            };
            if row.id() != id {
                return Err(Violation::RowIdMismatch { id });
            }
            row.check(self.cols, interner)?;
        }
        if let Some(interner) = interner {
            for cursor in [&self.cursor, &self.saved_cursor] {
                for cell in [cursor.template, cursor.erase] {
                    check_cell_ids(cell, interner).map_err(|kind| kind.at(cursor.pos.row, 0))?;
                }
            }
        }
        if cfg!(feature = "vt-paranoid") {
            self.check_stale_slots()?;
        }
        Ok(())
    }

    /// Every slot holds nothing, or a live row that maps to it.
    fn check_stale_slots(&self) -> Result<(), Violation> {
        for (slot, row) in self.slots.iter().enumerate() {
            let Some(row) = row else { continue };
            let id = row.id();
            let live = id >= self.oldest && id <= self.newest;
            if !live || self.slot_index(id) != slot {
                return Err(Violation::StaleSlot { slot });
            }
        }
        Ok(())
    }
}
