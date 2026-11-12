//! Images on the terminal: placing decoded pixels at the cursor and reclaiming extras ids.
//!
//! The bookkeeping that needs no line feed (footprints, stamping, release causes, the pixel
//! queue) lives in `crate::graphics`; the cursor walk runs the dispatcher's own line feed so
//! scroll reports, events and counters match what the same line feeds would have produced.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use crate::graphics::placement::{cells_for, effective_cell, stamp_row};
use crate::graphics::sixel::DecodedSixel;
use crate::graphics::{GraphicData, Placement};
use crate::grid::AnchorKind;
use crate::intern::{Extras, ExtrasId, GraphicId};

use super::State;
use super::dispatch::Handler;

/// The pixels a new placement shows.
pub(super) enum Picture {
    /// Fresh pixels that belong to this placement alone: they take its id.
    Own { size: (u32, u32), rgba: Vec<u8> },
    /// An image with an id of its own, which other placements may show too. `deliver` hands
    /// it to the embedder, who has not seen it yet.
    #[cfg(feature = "kitty-graphics")]
    Shared {
        data: Arc<GraphicData>,
        deliver: bool,
    },
}

/// How a picture sits on the grid; each protocol has its own rules for it.
pub(super) struct Layout {
    /// Columns covered, already clipped at the right edge of the screen.
    pub(super) cols: u16,
    /// Rows covered.
    pub(super) rows: u16,
    /// Line feeds the cursor makes while the rows are stamped.
    pub(super) walk: u16,
    pub(super) source_origin: (u32, u32),
    pub(super) source_size: (u32, u32),
    pub(super) pixel_offset: (u16, u16),
    pub(super) display_size: (u32, u32),
    pub(super) z_index: i32,
}

impl State {
    /// Interns `value` in the extras table, first freeing the ids no cell names when the
    /// table has taken enough new entries since the last sweep.
    ///
    /// The sweep runs *before* the intern: the value about to be interned is in no cell yet,
    /// so a sweep after it would free the fresh id.
    pub(super) fn intern_extras(&mut self, value: Extras) -> ExtrasId {
        self.sweep_extras_if_due();
        self.interner.extras(value)
    }

    /// Frees every extras id that no cell, pen or saved cursor names, if a sweep is due.
    pub(super) fn sweep_extras_if_due(&mut self) {
        if !self.interner.extras.needs_sweep() {
            return;
        }
        self.grid.live_extras_ids(&mut self.live_ids);
        self.interner.extras.sweep_unreferenced(&self.live_ids);
    }

    /// Frees every hyperlink that no cell, pen or saved cursor names, when the table is about
    /// to refuse a link and enough new ones arrived since the last sweep. Call it before the
    /// intern: the link about to be interned is in no cell yet.
    pub(super) fn sweep_hyperlinks_if_due(&mut self) {
        if !self.interner.hyperlinks.needs_sweep() {
            return;
        }
        self.grid.live_extras_ids(&mut self.live_ids);
        let interner = &self.interner;
        let live: FxHashSet<u32> = self
            .live_ids
            .iter()
            .filter_map(|id| u16::try_from(*id).ok())
            .filter_map(|id| interner.resolve_extras(ExtrasId(id)).hyperlink)
            .map(|link| link.0)
            .collect();
        self.interner.hyperlinks.sweep_unreferenced(&live);
        // A freed id names another link from now on: snapshot states drop the links they cached.
        self.bump_generation();
    }
}

impl Handler<'_> {
    /// Places a decoded Sixel image at the cursor: the footprint is what the pixels need, and
    /// the cursor walks `band_pixels / cell_height` rows (the DEC rule).
    pub(super) fn place_sixel(&mut self, image: DecodedSixel) {
        let cell = effective_cell(self.state.cell_pixels);
        // `effective_cell` never returns a zero axis.
        let walk = u16::try_from(image.band_pixels / u32::from(cell.1)).unwrap_or(u16::MAX);
        let room = {
            let screen = self.state.grid.screen();
            screen.cols().saturating_sub(screen.cursor().pos.col)
        };
        let size = (image.width, image.height);
        let (cols, rows) = cells_for(size, cell, room);
        let layout = Layout {
            cols,
            rows,
            walk,
            source_origin: (0, 0),
            source_size: size,
            pixel_offset: (0, 0),
            display_size: size,
            z_index: 0,
        };
        let rgba = image.rgba;
        self.place(Picture::Own { size, rgba }, layout);
    }

    /// Places `picture` at the cursor of the active screen and returns the id of the
    /// placement.
    ///
    /// The cursor walks `layout.walk` rows through the line feed (column kept), the cells of
    /// the footprint name the placement, and pixels the embedder has not seen are queued.
    pub(super) fn place(&mut self, picture: Picture, layout: Layout) -> GraphicId {
        let Layout {
            cols, rows, walk, ..
        } = layout;
        let cursor = self.state.grid.screen().cursor().pos;
        let id = self.state.graphics.next_graphic_id();
        let (image, pixel_size) = match &picture {
            Picture::Own { size, .. } => (id, *size),
            #[cfg(feature = "kitty-graphics")]
            Picture::Shared { data, .. } => (data.id, (data.width, data.height)),
        };
        let only = self.state.intern_extras(Extras {
            hyperlink: None,
            graphic: Some(id),
        });

        let State { grid, graphics, .. } = &mut *self.state;
        let anchor = grid.anchors_mut().register(AnchorKind::Graphic(id), cursor);
        let placement = Placement {
            id,
            image,
            anchor,
            cols,
            rows,
            pixel_size,
            source_origin: layout.source_origin,
            source_size: layout.source_size,
            pixel_offset: layout.pixel_offset,
            display_size: layout.display_size,
            z_index: layout.z_index,
        };
        graphics.admit(placement, grid);

        for step in 0..rows.max(walk.saturating_add(1)) {
            if step < rows {
                // While walking, the cursor row is the image row `step`; past the walk it
                // stays on the last walked row and the image continues below it.
                let State { grid, interner, .. } = &mut *self.state;
                let line = grid.screen().cursor().pos.row + u64::from(step.saturating_sub(walk));
                let span = cursor.col..cursor.col + cols;
                stamp_row(grid.screen_mut(), interner, line, span, id, only);
            }
            if step < walk {
                self.line_feed();
            }
        }

        let State {
            grid,
            graphics,
            stats,
            ..
        } = &mut *self.state;
        graphics.release_covered(grid);
        let unseen = match picture {
            Picture::Own { size, rgba } => Some(Arc::new(GraphicData {
                id,
                width: size.0,
                height: size.1,
                rgba,
            })),
            #[cfg(feature = "kitty-graphics")]
            Picture::Shared {
                data,
                deliver: true,
            } => Some(data),
            #[cfg(feature = "kitty-graphics")]
            Picture::Shared { .. } => None,
        };
        if let Some(data) = unseen {
            let dropped = graphics.push_pending(data, grid);
            stats.graphics_dropped = stats.graphics_dropped.saturating_add(dropped);
        }
        id
    }
}
