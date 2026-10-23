//! Rows of cells in a ring-buffer scrollback, primary and alternate screens, and the
//! primitives a dispatcher calls to edit them.
//!
//! # Screen and viewport
//!
//! The **screen** is the bottom `rows` rows of a [`Screen`]; every VT operation addresses it
//! by index. The **viewport** is what the user sees: `offset` rows above the screen, so
//! scrolling back never moves the cursor or the content. Nothing here returns an error or
//! panics: counts clamp and out-of-range reads give blanks.
//!
//! # Row ids
//!
//! A [`RowId`] names a **position in the output stream**; the ring slot is `id & mask`.
//! Content moves between ids in region scrolls, while a whole-screen scroll moves none.
//! Consumers follow content with anchors ([`Anchors`]), never by storing a `RowId`.
//!
//! - Ids only grow per lane; RIS, ED 2 and ED 3 never reset them, and a live id is never reused.
//! - Two lanes share one `u64` space: the primary screen counts from
//!   [`RowId::PRIMARY_ORIGIN`], the alternate from [`RowId::ALT_ORIGIN`]. The screens grow
//!   independently, so one counter could not keep both contiguous.
//! - [`RowId::distance`] saturates, so an out-of-order pair gives 0, never a wrap.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "crate-private grid operations are dispatched by Terminal from phase 6"
    )
)]

use std::ops::{Add, Sub};

mod anchor;
mod integrity;
mod row;
mod screen;
mod terminal_grid;

#[cfg(test)]
mod grid_fixture;

pub use anchor::{Anchor, AnchorId, AnchorKind, Anchors};
pub use row::{Row, RowFlags, RowHeader, RowMut, RowRef, SeqNo};
pub use screen::{
    Charset, Cursor, CursorOrigin, DisplayClear, LineClear, PrintMode, RowsScrolled, Screen,
    ScreenKind, ScrollReport, TabStops,
};
pub use terminal_grid::TerminalGrid;

/// Most scrollback rows a screen keeps, whatever limit is asked for.
pub const SCROLLBACK_MAX: u32 = 1_000_000;
/// The scrollback limit a terminal starts with.
pub const DEFAULT_SCROLLBACK: u32 = 10_000;
/// Tallest screen; sizes come from remote hosts, so every constructor clamps.
pub(crate) const MAX_ROWS: u16 = 1024;
/// Widest screen.
pub(crate) const MAX_COLS: u16 = 2048;

/// The identity of one row position in the output stream.
///
/// See the module documentation for the rules.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct RowId(pub u64);

impl RowId {
    /// The first id of the primary screen's lane.
    pub const PRIMARY_ORIGIN: RowId = RowId(0);
    /// The first id of the alternate screen's lane.
    pub const ALT_ORIGIN: RowId = RowId(1 << 63);

    /// Returns how many rows `self` is after `earlier`; 0 if it is not after it.
    pub fn distance(self, earlier: RowId) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

impl Add<u64> for RowId {
    type Output = RowId;

    fn add(self, rows: u64) -> RowId {
        RowId(self.0.saturating_add(rows))
    }
}

impl Sub<u64> for RowId {
    type Output = RowId;

    fn sub(self, rows: u64) -> RowId {
        RowId(self.0.saturating_sub(rows))
    }
}

/// A cell position: a row id and a column.
///
/// Positions order row-major.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Pos {
    /// The row.
    pub row: RowId,
    /// The zero-based column.
    pub col: u16,
}

/// A screen size in cells.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Size {
    /// Rows.
    pub rows: u16,
    /// Columns.
    pub cols: u16,
}

impl Size {
    /// Returns the size limited to `1..=1024` rows and `1..=2048` columns.
    pub fn clamped(self) -> Size {
        Size {
            rows: self.rows.clamp(1, MAX_ROWS),
            cols: self.cols.clamp(1, MAX_COLS),
        }
    }
}

/// The rows the user currently sees: `rows` rows from `top`, each `cols` wide.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Viewport {
    /// The id of the first visible row.
    pub top: RowId,
    /// Visible rows.
    pub rows: u16,
    /// Columns.
    pub cols: u16,
}

/// A scroll region as screen indices; `bottom` is exclusive.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScrollRegion {
    /// The first row of the region.
    pub top: u16,
    /// One past the last row of the region.
    pub bottom: u16,
}

impl ScrollRegion {
    /// The region covering a whole screen of `rows` rows.
    pub(crate) fn full(rows: u16) -> ScrollRegion {
        ScrollRegion {
            top: 0,
            bottom: rows,
        }
    }

    /// Rows in the region.
    pub(crate) fn height(self) -> u16 {
        self.bottom.saturating_sub(self.top)
    }

    /// Whether the screen index lies inside the region.
    pub(crate) fn contains(self, index: u16) -> bool {
        (self.top..self.bottom).contains(&index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_id_arithmetic_saturates() {
        assert_eq!(RowId(u64::MAX) + 5, RowId(u64::MAX));
        assert_eq!(RowId(3) - 5, RowId(0));
        assert_eq!(RowId(3).distance(RowId(9)), 0);
        assert_eq!(RowId(9).distance(RowId(3)), 6);
    }

    #[test]
    fn lanes_are_disjoint() {
        assert!(RowId::PRIMARY_ORIGIN < RowId::ALT_ORIGIN);
        assert_eq!(RowId::ALT_ORIGIN.0, 1 << 63);
    }

    #[test]
    fn pos_orders_row_major() {
        let a = Pos {
            row: RowId(1),
            col: 9,
        };
        let b = Pos {
            row: RowId(2),
            col: 0,
        };
        assert!(a < b);
    }

    #[test]
    fn size_clamps_both_ways() {
        let big = Size {
            rows: u16::MAX,
            cols: u16::MAX,
        };
        assert_eq!(
            big.clamped(),
            Size {
                rows: MAX_ROWS,
                cols: MAX_COLS
            }
        );
        assert_eq!(
            Size { rows: 0, cols: 0 }.clamped(),
            Size { rows: 1, cols: 1 }
        );
    }

    #[test]
    fn scroll_region_helpers() {
        let region = ScrollRegion { top: 2, bottom: 5 };
        assert_eq!(region.height(), 3);
        assert!(region.contains(2) && region.contains(4) && !region.contains(5));
        assert_eq!(ScrollRegion::full(7).height(), 7);
        assert_eq!(ScrollRegion { top: 5, bottom: 2 }.height(), 0);
    }
}
