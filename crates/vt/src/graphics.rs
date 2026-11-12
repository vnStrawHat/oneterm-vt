//! Images: Sixel and the kitty graphics protocol decoded to RGBA, anchored to the cells they
//! cover, released when those cells are gone.
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
//! A decoder produces RGBA pixels and nothing else; format-specific knowledge stops at
//! [`sixel`] and, with the `kitty-graphics` feature, at `kitty`.

use std::sync::Arc;

use crate::events::{EventBatch, VtEvent};
use crate::grid::AnchorId;
use crate::intern::GraphicId;

use self::sixel::SixelDecoder;

#[cfg(feature = "kitty-graphics")]
pub(crate) mod image_store;
#[cfg(feature = "kitty-graphics")]
pub(crate) mod kitty;
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
/// Most pixel bytes of live placements together; the oldest are released to make room. Without
/// it 256 placements of 4096 x 4096 pixels would be 16 GiB of textures an embedder must keep.
pub(crate) const LIVE_MAX_BYTES: usize = 256 << 20;

/// A decoded image: RGBA8, row-major, stride `width * 4`, straight (not premultiplied) alpha.
///
/// `width` and `height` are at most 4096; `rgba.len() == width * height * 4`. Pixels the
/// image never painted are transparent (`[0, 0, 0, 0]`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GraphicData {
    /// The image: what [`Placement::image`] names.
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
///
/// To draw it, take the pixels of `image` (a [`GraphicData`]), cut out `source_size` pixels
/// at `source_origin`, stretch them to `display_size` and put them at the origin of the
/// top-left cell plus `pixel_offset`. A Sixel image is its own placement (`image == id`),
/// is shown whole at its own size, without offset, on layer 0.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    /// The placement: the id its cells carry.
    pub id: GraphicId,
    /// The pixels it shows: the id of a [`GraphicData`]. Several placements of one kitty
    /// image share it; for a Sixel image it equals `id`.
    pub image: GraphicId,
    /// The anchor of the top-left cell.
    pub anchor: AnchorId,
    /// Columns covered, clipped to the screen width at placement time.
    pub cols: u16,
    /// Rows covered.
    pub rows: u16,
    /// The size of the whole image in pixels, `(width, height)`.
    pub pixel_size: (u32, u32),
    /// The top-left corner of the part of the image that is shown, in image pixels.
    pub source_origin: (u32, u32),
    /// The size of that part, in image pixels.
    pub source_size: (u32, u32),
    /// Where the picture starts inside its top-left cell, in pixels.
    pub pixel_offset: (u16, u16),
    /// The size to draw the shown part at, in device pixels of the cell size it was placed
    /// with.
    pub display_size: (u32, u32),
    /// The layer: higher is drawn later, and a layer below 0 goes under the text.
    pub z_index: i32,
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
    pub(crate) placements: placement::PlacementList,
    /// The RGBA bytes of the images `placements` show, each image counted once.
    pub(crate) live_bytes: usize,
    /// Ids released since the last feed delivered them, in release order.
    pub(crate) released: Vec<GraphicId>,
    /// The open `DCS q` string, if any.
    pub(crate) decoder: Option<SixelDecoder>,
    /// The kitty graphics protocol: the `APC` reader, the stored images, who placed what.
    #[cfg(feature = "kitty-graphics")]
    pub(crate) kitty: kitty::KittyState,
}

impl GraphicsState {
    pub(crate) fn new() -> GraphicsState {
        GraphicsState {
            next_id: 1,
            pending: Vec::new(),
            pending_bytes: 0,
            placements: placement::PlacementList::default(),
            live_bytes: 0,
            released: Vec::new(),
            decoder: None,
            #[cfg(feature = "kitty-graphics")]
            kitty: kitty::KittyState::default(),
        }
    }

    /// The next image or placement id; ids are never reused.
    pub(crate) fn next_graphic_id(&mut self) -> GraphicId {
        let id = GraphicId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// RIS: forgets queued pixels and the open decoder. Placements stay until the end-of-feed
    /// sweep finds their rows reset, so each is released exactly once; the id counter stays.
    pub(crate) fn reset(&mut self) {
        self.pending.clear();
        self.pending_bytes = 0;
        self.decoder = None;
        #[cfg(feature = "kitty-graphics")]
        {
            let forgotten = self.kitty.reset();
            self.retire(forgotten);
        }
    }

    /// Moves every pending release into `batch` as `GraphicReleased` events, in order.
    pub(crate) fn drain_released(&mut self, batch: &mut EventBatch) {
        for id in self.released.drain(..) {
            batch.push(VtEvent::GraphicReleased(id));
        }
    }
}
