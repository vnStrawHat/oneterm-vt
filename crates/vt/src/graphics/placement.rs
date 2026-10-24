//! Placement bookkeeping: footprints, stamping cells, the release causes, the pixel queue.
//!
//! The pieces that need the terminal's line feed (the cursor walk) live with the dispatcher;
//! everything here takes the grid it works on explicitly.

use std::ops::Range;
use std::sync::Arc;

use super::{GraphicData, GraphicsState, PENDING_MAX_BYTES, VIRTUAL_CELL};
use crate::grid::{AnchorId, RowFlags, RowId, Screen, TerminalGrid};
use crate::intern::{ExtrasId, GraphicId, Interner};

/// The cell size an image is measured against: `cell_pixels` when both axes are set, else
/// [`VIRTUAL_CELL`]. A half-set size falls back whole, so nothing divides by zero.
pub(crate) fn effective_cell(cell_pixels: (u16, u16)) -> (u16, u16) {
    match cell_pixels {
        (width, height) if width > 0 && height > 0 => (width, height),
        _ => VIRTUAL_CELL,
    }
}

/// The footprint of an image of `size` pixels on cells of `cell` pixels: `ceil(px / cell)`
/// per axis, columns clipped to `max_cols`, at least one row. Both saturate at `u16::MAX`.
pub(crate) fn cells_for(size: (u32, u32), cell: (u16, u16), max_cols: u16) -> (u16, u16) {
    let across = size.0.div_ceil(u32::from(cell.0.max(1)));
    let down = size.1.div_ceil(u32::from(cell.1.max(1))).max(1);
    let cols = u16::try_from(across).unwrap_or(u16::MAX).min(max_cols);
    (cols, u16::try_from(down).unwrap_or(u16::MAX))
}

/// Marks the cells `cols` of row `line` as covered by image `id`: an empty cell takes the
/// image's own entry, a cell that already has extras (a hyperlink, an older image) gets a
/// merged entry that keeps its hyperlink. Text, style and wrap flag stay.
///
/// The merged entry is interned without a sweep: the row is borrowed and the id is written
/// before anything can sweep. A row outside the screen is skipped.
pub(crate) fn stamp_row(
    screen: &mut Screen,
    interner: &mut Interner,
    line: RowId,
    cols: Range<u16>,
    id: GraphicId,
    only: ExtrasId,
) {
    let Some(mut row) = screen.row_mut(line) else {
        return;
    };
    let mut stamped = false;
    for col in cols {
        let Some(cell) = row.cells().get(usize::from(col)).copied() else {
            break;
        };
        let current = cell.extras_id();
        let extras = if current == ExtrasId::NONE {
            only
        } else {
            let mut merged = *interner.resolve_extras(current);
            merged.graphic = Some(id);
            interner.extras(merged)
        };
        // A full extras table answers NONE; writing it would erase the cell's other extras.
        if extras == ExtrasId::NONE {
            continue;
        }
        row.repair(col, cell.with_extras(extras));
        stamped = true;
    }
    if stamped {
        row.mark_graphic();
    }
}

/// Where a placement is, as cell ranges.
struct Extent {
    top: u64,
    bottom: u64,
    left: u32,
    right: u32,
}

impl Extent {
    fn contains(&self, inner: &Extent) -> bool {
        self.top <= inner.top
            && inner.bottom <= self.bottom
            && self.left <= inner.left
            && inner.right <= self.right
    }
}

impl GraphicsState {
    /// Removes placement `index` and queues its `GraphicReleased`.
    pub(crate) fn release(&mut self, index: usize, grid: &mut TerminalGrid) {
        if index >= self.placements.len() {
            return;
        }
        let placement = self.placements.remove(index);
        grid.anchors_mut().release(placement.anchor);
        self.released.push(placement.id);
    }

    fn extent(&self, index: usize, grid: &TerminalGrid) -> Option<Extent> {
        let placement = self.placements.get(index)?;
        let pos = grid.anchors().get(placement.anchor)?;
        Some(Extent {
            top: pos.row.0,
            bottom: pos.row.0.saturating_add(u64::from(placement.rows)),
            left: u32::from(pos.col),
            right: u32::from(pos.col) + u32::from(placement.cols),
        })
    }

    /// Releases every older placement whose extent lies inside the newest one's: a redrawn
    /// frame replaces its predecessor at once instead of waiting for the rows to go.
    pub(crate) fn release_covered(&mut self, grid: &mut TerminalGrid) {
        let Some(newest) = self.placements.len().checked_sub(1) else {
            return;
        };
        let Some(outer) = self.extent(newest, grid) else {
            return;
        };
        // The newest placement stays last while older ones are removed before it.
        let mut index = 0;
        while index + 1 < self.placements.len() {
            let covered = self
                .extent(index, grid)
                .is_some_and(|inner| outer.contains(&inner));
            if covered {
                self.release(index, grid);
            } else {
                index += 1;
            }
        }
    }

    /// Releases the placements that are no longer live: the anchor died, or no row of the
    /// extent that exists still carries the `HAS_GRAPHIC` hint.
    pub(crate) fn sweep(&mut self, grid: &mut TerminalGrid) {
        let mut index = 0;
        while let Some(placement) = self.placements.get(index) {
            if is_live(placement.rows, placement.anchor, grid) {
                index += 1;
            } else {
                self.release(index, grid);
            }
        }
    }

    /// Queues `image` for `take_graphics`. While the queue would pass its byte ceiling the
    /// oldest pending image is dropped and its placement released; returns how many.
    pub(crate) fn push_pending(&mut self, image: Arc<GraphicData>, grid: &mut TerminalGrid) -> u32 {
        let bytes = image.rgba.len().max(std::mem::size_of::<GraphicData>());
        let mut dropped = 0u32;
        while !self.pending.is_empty()
            && self.pending_bytes.saturating_add(bytes) > PENDING_MAX_BYTES
        {
            let oldest = self.pending.remove(0);
            self.pending_bytes = self
                .pending_bytes
                .saturating_sub(oldest.rgba.len().max(std::mem::size_of::<GraphicData>()));
            if let Some(index) = self.placements.iter().position(|p| p.id == oldest.id) {
                self.release(index, grid);
            }
            dropped = dropped.saturating_add(1);
        }
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        self.pending.push(image);
        dropped
    }
}

/// Whether a placement of `rows` rows anchored at `anchor` still shows an image: its anchor is
/// alive and a row of its extent that exists carries `HAS_GRAPHIC`.
fn is_live(rows: u16, anchor: AnchorId, grid: &TerminalGrid) -> bool {
    let Some(pos) = grid.anchors().get(anchor) else {
        return false;
    };
    let screen = grid.screen_of(pos.row);
    let live = screen.row_range();
    let mut id = pos.row.max(live.start);
    let end = (pos.row + u64::from(rows)).min(live.end);
    while id < end {
        if screen.row(id).flags().contains(RowFlags::HAS_GRAPHIC) {
            return true;
        }
        id = id + 1;
    }
    false
}
