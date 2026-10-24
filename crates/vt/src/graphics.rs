//! Images: Sixel decoded to RGBA, anchored to the cells they cover, released when those cells
//! are gone.
//!
//! Three rules shape everything here:
//!
//! 1. A cell stores *which* image (one interned `Extras { graphic }` per image), never where
//!    inside it. A [`Placement`] is the anchor of the image's top-left cell.
//! 2. A placement is an anchor (`AnchorKind::Graphic`), so it moves with its content through
//!    every row-moving primitive and through reflow.
//! 3. Liveness is derived from rows, not counted: a placement lives while its anchor lives and
//!    a row of its extent still carries the `HAS_GRAPHIC` hint. A counter would miss row resets,
//!    blanking scrolls, alternate-screen wipes and reflow.
//!
//! The decoder produces RGBA pixels and nothing else; format-specific knowledge stops at
//! [`sixel`].

use std::sync::Arc;

use crate::events::{EventBatch, VtEvent};
use crate::grid::AnchorId;
use crate::intern::GraphicId;

use self::sixel::SixelDecoder;

pub(crate) mod placement;
pub(crate) mod sixel;

#[cfg(test)]
mod graphics_tests;

/// The cell size, in pixels, used to size an image while the embedder has not set one with
/// `Terminal::set_cell_pixels`: `(width, height)`, the VT340 cell.
pub const VIRTUAL_CELL: (u16, u16) = (10, 20);

/// Longest image side the decoder keeps, in pixels.
pub(crate) const MAX_DIMENSION: u32 = 4096;
/// Most live placements; the oldest is released to make room.
pub(crate) const MAX_PLACEMENTS: usize = 256;
/// Most pixel bytes waiting for `Terminal::take_graphics`.
pub(crate) const PENDING_MAX_BYTES: usize = 128 << 20;

/// A decoded image: RGBA8, row-major, stride `width * 4`, straight (not premultiplied) alpha.
///
/// `width` and `height` are at most 4096; `rgba.len() == width * height * 4`. Pixels the
/// image never painted are transparent (`[0, 0, 0, 0]`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GraphicData {
    /// The image; the same id names its [`Placement`] and its cells.
    pub id: GraphicId,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels, four bytes each.
    pub rgba: Vec<u8>,
}

/// An image placed on the grid. Read-only: no API takes one.
///
/// `anchor` follows the image's top-left cell (read it with the grid's anchors, or through
/// `SnapshotState::placements`, which resolves it). `cols` is already clipped at the right
/// edge; `rows` is the full height and is not clipped at the bottom. The footprint is fixed
/// when the image is placed: later `set_cell_pixels` calls do not change it.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    /// The image.
    pub id: GraphicId,
    /// The anchor of the top-left cell.
    pub anchor: AnchorId,
    /// Columns covered, clipped to the screen width at placement time.
    pub cols: u16,
    /// Rows covered.
    pub rows: u16,
    /// The decoded size in pixels, `(width, height)`.
    pub pixel_size: (u32, u32),
}

/// The graphics side of a terminal's state.
pub(crate) struct GraphicsState {
    /// The next image id; starts at 1 and is never reused.
    pub(crate) next_id: u64,
    /// Decoded images waiting for `take_graphics`, oldest first.
    pub(crate) pending: Vec<Arc<GraphicData>>,
    /// The RGBA bytes held by `pending`.
    pub(crate) pending_bytes: usize,
    /// Live placements, oldest first (draw order).
    pub(crate) placements: Vec<Placement>,
    /// Ids released since the last feed delivered them, in release order.
    pub(crate) released: Vec<GraphicId>,
    /// The open `DCS q` string, if any.
    pub(crate) decoder: Option<SixelDecoder>,
}

impl GraphicsState {
    pub(crate) fn new() -> GraphicsState {
        GraphicsState {
            next_id: 1,
            pending: Vec::new(),
            pending_bytes: 0,
            placements: Vec::new(),
            released: Vec::new(),
            decoder: None,
        }
    }

    /// RIS: forgets queued pixels and the open decoder. Placements stay until the end-of-feed
    /// sweep finds their rows reset, so each is released exactly once; the id counter stays.
    pub(crate) fn reset(&mut self) {
        self.pending.clear();
        self.pending_bytes = 0;
        self.decoder = None;
    }

    /// Moves every pending release into `batch` as `GraphicReleased` events, in order.
    pub(crate) fn drain_released(&mut self, batch: &mut EventBatch) {
        for id in self.released.drain(..) {
            batch.push(VtEvent::GraphicReleased(id));
        }
    }
}
