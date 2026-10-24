//! Images on the terminal: placing a decoded Sixel at the cursor and reclaiming extras ids.
//!
//! The bookkeeping that needs no line feed (footprints, stamping, release causes, the pixel
//! queue) lives in `crate::graphics`; the cursor walk runs the dispatcher's own line feed so
//! scroll reports, events and counters match what the same line feeds would have produced.

use std::sync::Arc;

use crate::graphics::placement::{cells_for, effective_cell, stamp_row};
use crate::graphics::sixel::DecodedSixel;
use crate::graphics::{GraphicData, MAX_PLACEMENTS, Placement};
use crate::grid::AnchorKind;
use crate::intern::{Extras, ExtrasId, GraphicId};

use super::State;
use super::dispatch::Handler;

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
        self.grid.live_extras_ids(&mut self.live_extras);
        self.interner.extras.sweep_unreferenced(&self.live_extras);
    }
}

impl Handler<'_> {
    /// Places a decoded image at the cursor of the active screen.
    ///
    /// The cursor walks `band_pixels / cell_height` rows through the line feed (column kept),
    /// the cells of the footprint name the image, and the placement is queued for the embedder.
    pub(super) fn place(&mut self, image: DecodedSixel) {
        let cell = effective_cell(self.state.cell_pixels);
        // `effective_cell` never returns a zero axis.
        let walk = u16::try_from(image.band_pixels / u32::from(cell.1)).unwrap_or(u16::MAX);
        let (cursor, screen_cols) = {
            let screen = self.state.grid.screen();
            (screen.cursor().pos, screen.cols())
        };
        let size = (image.width, image.height);
        let (cols, rows) = cells_for(size, cell, screen_cols.saturating_sub(cursor.col));
        let id = GraphicId(self.state.graphics.next_id);
        self.state.graphics.next_id = self.state.graphics.next_id.saturating_add(1);
        let only = self.state.intern_extras(Extras {
            hyperlink: None,
            graphic: Some(id),
        });

        let State { grid, graphics, .. } = &mut *self.state;
        while graphics.placements.len() >= MAX_PLACEMENTS {
            graphics.release(0, grid);
        }
        let anchor = grid.anchors_mut().register(AnchorKind::Graphic(id), cursor);
        graphics.placements.push(Placement {
            id,
            anchor,
            cols,
            rows,
            pixel_size: size,
        });

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
        let data = Arc::new(GraphicData {
            id,
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        });
        let dropped = graphics.push_pending(data, grid);
        stats.graphics_dropped = stats.graphics_dropped.saturating_add(dropped);
    }
}
