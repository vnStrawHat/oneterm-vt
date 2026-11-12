//! Placement bookkeeping: footprints, stamping cells, the release causes, the pixel queue.
//!
//! The pieces that need the terminal's line feed (the cursor walk) live with the dispatcher;
//! everything here takes the grid it works on explicitly.

use std::ops::Range;
use std::sync::Arc;

use super::{
    GraphicData, GraphicsState, LIVE_MAX_BYTES, MAX_PLACEMENTS, PENDING_MAX_BYTES, Placement,
    VIRTUAL_CELL,
};
use crate::grid::{AnchorId, RowFlags, RowId, Screen, TerminalGrid};
use crate::intern::{ExtrasId, GraphicId, Interner};

#[cfg(feature = "kitty-graphics")]
use super::image_store::StoredImage;

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

/// Where a placement is, as cell ranges: rows `top..bottom`, columns `left..right`.
pub(crate) struct Extent {
    pub(crate) top: u64,
    pub(crate) bottom: u64,
    pub(crate) left: u32,
    pub(crate) right: u32,
}

impl Extent {
    fn contains(&self, inner: &Extent) -> bool {
        self.top <= inner.top
            && inner.bottom <= self.bottom
            && self.left <= inner.left
            && inner.right <= self.right
    }
}

/// The RGBA bytes of an image of `size` pixels.
fn pixel_bytes(size: (u32, u32)) -> usize {
    (size.0 as usize)
        .saturating_mul(size.1 as usize)
        .saturating_mul(4)
}

/// The live placements, oldest first (draw order).
///
/// The oldest one is the one that usually goes (a stream of images that scroll away evicts
/// it at the placement ceiling), so the front is dropped by moving a start index; the dead
/// front is cut off once it is as long as the live part.
#[derive(Debug, Default)]
pub(crate) struct PlacementList {
    items: Vec<Placement>,
    /// The index of the oldest live placement in `items`.
    start: usize,
}

impl std::ops::Deref for PlacementList {
    type Target = [Placement];

    fn deref(&self) -> &[Placement] {
        self.items.get(self.start..).unwrap_or(&[])
    }
}

impl PlacementList {
    /// Adds `placement` as the newest one.
    fn push(&mut self, placement: Placement) {
        self.items.push(placement);
    }

    /// Takes placement `index` out; `None` when there is none.
    fn remove(&mut self, index: usize) -> Option<Placement> {
        let at = self.start + index;
        let placement = *self.items.get(at)?;
        if index > 0 {
            self.items.remove(at);
            return Some(placement);
        }
        self.start += 1;
        if self.start * 2 >= self.items.len() {
            self.items.drain(..self.start);
            self.start = 0;
        }
        Some(placement)
    }
}

impl GraphicsState {
    /// Whether a live placement shows the image `image`.
    fn shows(&self, image: GraphicId) -> bool {
        self.placements.iter().any(|p| p.image == image)
    }

    /// Whether a live placement other than `placement` (which is not in the list) shows its
    /// image. A picture that is its own image, as every Sixel one, is never shared, so the
    /// common case walks nothing.
    fn shared(&self, placement: &Placement) -> bool {
        placement.image != placement.id && self.shows(placement.image)
    }

    /// Adds `placement` as the newest one, first releasing the oldest placements until it fits
    /// both the placement count and the pixel budget of live images. An image that another
    /// placement already shows costs nothing more.
    pub(crate) fn admit(&mut self, placement: Placement, grid: &mut TerminalGrid) {
        let bytes = pixel_bytes(placement.pixel_size);
        loop {
            // Asked again each round: a release may have ended the last other placement.
            let added = if self.shared(&placement) { 0 } else { bytes };
            let fits = self.placements.len() < MAX_PLACEMENTS
                && self.live_bytes.saturating_add(added) <= LIVE_MAX_BYTES;
            if fits || self.placements.is_empty() {
                self.live_bytes = self.live_bytes.saturating_add(added);
                break;
            }
            self.release(0, grid);
        }
        self.placements.push(placement);
    }

    /// Removes placement `index` and queues its `GraphicReleased`; the image it showed
    /// follows when this was its last placement and the program cannot place it again.
    pub(crate) fn release(&mut self, index: usize, grid: &mut TerminalGrid) {
        let Some(placement) = self.placements.remove(index) else {
            return;
        };
        grid.anchors_mut().release(placement.anchor);
        self.released.push(placement.id);
        #[cfg(feature = "kitty-graphics")]
        self.kitty
            .refs
            .retain(|entry| entry.graphic != placement.id);
        if self.shared(&placement) {
            return;
        }
        self.live_bytes = self
            .live_bytes
            .saturating_sub(pixel_bytes(placement.pixel_size));
        #[cfg(feature = "kitty-graphics")]
        if placement.image != placement.id && !self.kitty.store.holds(placement.image) {
            self.released.push(placement.image);
        }
    }

    /// Queues `GraphicReleased` for the images the store gave up, so the embedder frees
    /// their pixels. One that a placement still shows goes with its last placement; one
    /// that was never handed out needs no event.
    #[cfg(feature = "kitty-graphics")]
    pub(crate) fn retire(&mut self, forgotten: impl IntoIterator<Item = StoredImage>) {
        for stored in forgotten {
            if stored.delivered && !self.shows(stored.data.id) {
                self.released.push(stored.data.id);
            }
        }
    }

    /// Releases the placement of image `id`, if it is live.
    #[cfg(feature = "kitty-graphics")]
    pub(crate) fn release_id(&mut self, id: GraphicId, grid: &mut TerminalGrid) {
        if let Some(index) = self.placements.iter().position(|p| p.id == id) {
            self.release(index, grid);
        }
    }

    /// Where the placement of image `id` is now.
    #[cfg(feature = "kitty-graphics")]
    pub(crate) fn extent_of(&self, id: GraphicId, grid: &TerminalGrid) -> Option<Extent> {
        extent_of(self.placements.iter().find(|p| p.id == id)?, grid)
    }

    fn extent(&self, index: usize, grid: &TerminalGrid) -> Option<Extent> {
        extent_of(self.placements.get(index)?, grid)
    }

    /// Releases every older placement whose extent lies inside that of the newest, unless it
    /// is on a layer above it: a redrawn frame replaces its predecessor at once instead of
    /// waiting for the rows to go.
    pub(crate) fn release_covered(&mut self, grid: &mut TerminalGrid) {
        let Some(newest) = self.placements.len().checked_sub(1) else {
            return;
        };
        let Some(outer) = self.extent(newest, grid) else {
            return;
        };
        let layer = self.placements.get(newest).map_or(0, |p| p.z_index);
        // Usually nothing is covered: one pass over the older placements finds that out.
        // The newest placement stays last while older ones are removed before it.
        let mut checked = 0;
        loop {
            let older = self
                .placements
                .split_last()
                .map_or(&[][..], |(_, older)| older);
            let rest = older.get(checked..).unwrap_or(&[]);
            let anchors = &*grid;
            let covered = |older: &Placement| {
                older.z_index <= layer
                    && extent_of(older, anchors).is_some_and(|inner| outer.contains(&inner))
            };
            let Some(found) = rest.iter().position(covered) else {
                return;
            };
            checked += found;
            self.release(checked, grid);
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
    /// oldest pending image is dropped and its placements released; returns how many images.
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
            while let Some(index) = self.placements.iter().position(|p| p.image == oldest.id) {
                self.release(index, grid);
            }
            // A stored image is queued again by its next placement.
            #[cfg(feature = "kitty-graphics")]
            self.kitty.store.mark_undelivered(oldest.id);
            dropped = dropped.saturating_add(1);
        }
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
        self.pending.push(image);
        dropped
    }
}

/// Where `placement` is now; `None` when its anchor is gone.
fn extent_of(placement: &Placement, grid: &TerminalGrid) -> Option<Extent> {
    let pos = grid.anchors().get(placement.anchor)?;
    Some(Extent {
        top: pos.row.0,
        bottom: pos.row.0.saturating_add(u64::from(placement.rows)),
        left: u32::from(pos.col),
        right: u32::from(pos.col) + u32::from(placement.cols),
    })
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
