//! `SnapshotState`: one consumer's damage-tracked view of one terminal.

use std::time::Instant;

use rustc_hash::FxHashMap;

use crate::grid::{RowId, RowRef, Screen, SeqNo, Size, TerminalGrid, Viewport};
use crate::intern::{GraphicId, Hyperlink, HyperlinkId, Interner};
use crate::selection::SelectionRange;

use super::mode_snapshot::ModeSnapshot;
use super::palette::Palette;
use super::row::SnapshotRow;
use super::sync::SyncState;

/// How far a consumer has read the grid's row stamps.
///
/// A row whose stamp is above the watermark changed since the consumer last copied it. The
/// watermark never decreases while a state is used with one terminal, and a suppressed
/// update (synchronized output) never moves it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Watermark(pub SeqNo);

/// What [`Terminal::snapshot_update`](crate::Terminal::snapshot_update) changed.
///
/// Whatever the result, [`SnapshotState::rows`] holds the whole viewport afterwards and the
/// cursor, selection and modes are current. A renderer that caches drawn rows reacts per result:
///
/// - `Unchanged`: nothing to redraw (rows may still be held back by synchronized output).
/// - `Partial { scrolled }`: shift the cache by `scrolled` viewport rows (positive: content
///   moved up and older rows left at the top; negative: it moved down), then redraw exactly
///   the rows in [`SnapshotState::changed`]. The cursor, selection or modes may have changed
///   even when `changed` is empty.
/// - `Full`: redraw everything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotUpdate {
    /// No row changed and neither did the cursor, selection or modes.
    Unchanged,
    /// Some rows changed, or only the cursor, selection or modes did.
    Partial {
        /// How many viewport rows the cached content moved up; negative moved down.
        scrolled: i32,
    },
    /// Rebuild every row.
    Full,
}

/// Where the cursor is, as of the last update.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SnapshotCursor {
    /// The cursor's row.
    pub id: RowId,
    /// The cursor's column.
    pub col: u16,
    /// The cursor's row within the viewport; `None` while the viewport is scrolled back past it.
    pub row: Option<u16>,
    /// Whether the cursor is shown (`?25`, [`ModeSnapshot::show_cursor`]).
    pub visible: bool,
}

/// An image placed on the grid, as a renderer sees it.
///
/// Images are not supported yet: [`SnapshotState::placements`] is always empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapshotPlacement {
    /// The image.
    pub id: GraphicId,
    /// The row of the image's top-left cell.
    pub row: RowId,
    /// The column of the image's top-left cell.
    pub col: u16,
    /// Columns covered.
    pub cols: u16,
    /// Rows covered.
    pub rows: u16,
    /// The image size in pixels, `(width, height)`.
    pub pixel_size: (u32, u32),
}

/// One consumer's view of one terminal, reused across frames.
///
/// Drawing takes two phases. Phase 1, [`Terminal::snapshot_update`](crate::Terminal::snapshot_update),
/// runs under the embedder's lock and copies only the rows that changed since this state's
/// watermark. Phase 2, [`map_colors`](Self::map_colors) and drawing, reads only the state, so
/// it can run outside the lock.
///
/// A state belongs to one terminal; every consumer (a window, a minimap) keeps its own, and
/// two states over one terminal never consume each other's damage. To point a state at
/// another terminal, call [`invalidate`](Self::invalidate) first. A state owns values only and
/// The watermark check only detects a grid younger than the state; a state reused across
/// terminals must call [`invalidate`](Self::invalidate) first, otherwise identical ids and
/// sequence numbers can hide stale rows.
/// is `Send` and `Sync`.
#[derive(Debug, Default)]
pub struct SnapshotState {
    /// The terminal's `generation` at the last copy; `None` until the first update.
    generation: Option<u32>,
    /// The terminal's `palette_epoch` at the last copy.
    palette_epoch: u32,
    /// The epoch `map_colors` last ran at.
    mapped_epoch: Option<u32>,
    watermark: Watermark,
    viewport_top: RowId,
    scroll_offset: u32,
    size: Option<Size>,
    rows: Vec<SnapshotRow>,
    changed: Vec<u16>,
    cursor: SnapshotCursor,
    selection: Option<SelectionRange>,
    modes: ModeSnapshot,
    placements: Vec<SnapshotPlacement>,
    hyperlinks: FxHashMap<HyperlinkId, Hyperlink>,
    /// Set by `invalidate`: the next real update is `Full`.
    force_full: bool,
    /// A cursor, mode or selection change that a suppressed update saw and nothing reported yet.
    meta_dirty: bool,
    rows_copied: u64,
}

/// What a terminal lends a state for one update.
pub(crate) struct EngineView<'a> {
    pub(crate) grid: &'a TerminalGrid,
    pub(crate) interner: &'a Interner,
    pub(crate) sync: &'a mut SyncState,
    pub(crate) modes: ModeSnapshot,
    pub(crate) generation: u32,
    pub(crate) palette_epoch: u32,
    pub(crate) selection: Option<SelectionRange>,
}

impl SnapshotState {
    /// An empty state; the first update is [`SnapshotUpdate::Full`].
    pub fn new() -> SnapshotState {
        SnapshotState::default()
    }

    /// The whole viewport after the first update, indexed by viewport row, whatever the last
    /// result was. Empty before the first update.
    pub fn rows(&self) -> &[SnapshotRow] {
        &self.rows
    }

    /// The viewport rows the last update copied, ascending; empty after
    /// [`SnapshotUpdate::Unchanged`]. With `Partial { scrolled }`, shift your cache first.
    pub fn changed(&self) -> &[u16] {
        &self.changed
    }

    /// The viewport size at the last copy; 0 x 0 before the first update.
    pub fn size(&self) -> Size {
        self.size.unwrap_or(Size { rows: 0, cols: 0 })
    }

    /// The id of the first visible row at the last copy.
    pub fn viewport_top(&self) -> RowId {
        self.viewport_top
    }

    /// How many rows lie between the viewport and the bottom at the last copy; 0 follows output.
    pub fn scroll_offset(&self) -> u32 {
        self.scroll_offset
    }

    /// The cursor, refreshed by every update.
    pub fn cursor(&self) -> &SnapshotCursor {
        &self.cursor
    }

    /// The selection, refreshed by every update.
    pub fn selection(&self) -> Option<SelectionRange> {
        self.selection
    }

    /// The modes, refreshed by every update (a held-back one included).
    pub fn modes(&self) -> ModeSnapshot {
        self.modes
    }

    /// How far this state has read the grid's row stamps.
    pub fn watermark(&self) -> Watermark {
        self.watermark
    }

    /// The link behind a cell's [`hyperlink`](crate::SnapshotCell::hyperlink); `None` if the
    /// id was not copied with any row.
    pub fn hyperlink(&self, id: HyperlinkId) -> Option<&Hyperlink> {
        self.hyperlinks.get(&id)
    }

    /// The images on the viewport. Always empty until images are supported.
    pub fn placements(&self) -> &[SnapshotPlacement] {
        &self.placements
    }

    /// The placement of image `id`, if it is on the viewport.
    pub fn placement(&self, id: GraphicId) -> Option<&SnapshotPlacement> {
        self.placements.iter().find(|placement| placement.id == id)
    }

    /// Which cell of image `id` the grid cell at `row` and `col` shows, as `(column, row)`
    /// offsets from the image's top-left cell; `None` outside the image or for an unknown id.
    pub fn graphic_offset(&self, id: GraphicId, row: RowId, col: u16) -> Option<(u16, u16)> {
        let placement = self.placement(id)?;
        let across = col
            .checked_sub(placement.col)
            .filter(|d| *d < placement.cols)?;
        let down = row
            .0
            .checked_sub(placement.row.0)
            .and_then(|d| u16::try_from(d).ok())
            .filter(|d| *d < placement.rows)?;
        Some((across, down))
    }

    /// How many rows this state has copied since it was created. It stays flat across idle
    /// updates.
    pub fn rows_copied(&self) -> u64 {
        self.rows_copied
    }

    /// Makes the next update that is not held back by synchronized output a
    /// [`SnapshotUpdate::Full`]. Call it before pointing the state at another terminal.
    pub fn invalidate(&mut self) {
        self.force_full = true;
    }

    /// Resolves every colour of the copied rows against `palette`, so drawing needs no
    /// terminal. Call it after every update, with the palette of the terminal's current
    /// colour epoch ([`Terminal::palette`](crate::Terminal::palette)).
    ///
    /// It maps every row when the epoch changed since the last call (a theme change through
    /// [`Terminal::set_theme_colors`](crate::Terminal::set_theme_colors), or an OSC colour
    /// change) and only the rows of [`changed`](Self::changed) otherwise, so a theme must go
    /// through that method or rows mapped earlier keep their old colours. Calling it again
    /// with the same palette changes nothing.
    pub fn map_colors(&mut self, palette: &Palette) {
        if self.mapped_epoch == Some(self.palette_epoch) {
            for &index in &self.changed {
                if let Some(row) = self.rows.get_mut(usize::from(index)) {
                    row.map_colors(palette);
                }
            }
        } else {
            for row in &mut self.rows {
                row.map_colors(palette);
            }
        }
        self.mapped_epoch = Some(self.palette_epoch);
    }

    /// Phase 1 of an update; see [`Terminal::snapshot_update`](crate::Terminal::snapshot_update).
    pub(crate) fn begin_update(&mut self, engine: EngineView<'_>, now: Instant) -> SnapshotUpdate {
        let EngineView {
            grid,
            interner,
            sync,
            modes,
            generation,
            palette_epoch,
            selection,
        } = engine;
        let screen = grid.screen();
        self.placements.clear();

        let cursor = snapshot_cursor(screen, modes.show_cursor);
        let meta_changed =
            self.modes != modes || self.cursor != cursor || self.selection != selection;
        self.modes = modes;
        self.cursor = cursor;
        self.selection = selection;

        // A state with no rows is built even inside a block: `rows` must hold the viewport
        // for every result.
        if sync.suppresses_frame(now) && !self.rows.is_empty() {
            self.meta_dirty |= meta_changed;
            self.changed.clear();
            return SnapshotUpdate::Unchanged;
        }

        let viewport = screen.viewport();
        let seq = grid.seq();
        let scrolled = self.viewport_delta(viewport.top);
        let rebuild = self.generation != Some(generation)
            || self.palette_epoch != palette_epoch
            || self.force_full
            || self.size != Some(screen.size())
            || self.rows.len() != usize::from(viewport.rows)
            || scrolled.is_none_or(|rows| rows.unsigned_abs() >= u32::from(viewport.rows))
            // A grid whose stamps are behind the watermark is not the one this state read.
            || seq < self.watermark.0;
        let scrolled = scrolled.unwrap_or(0);

        self.changed.clear();
        if rebuild {
            self.rebuild(screen, viewport, interner);
        } else {
            self.shift_rows(scrolled);
            self.copy_changed(screen, viewport.top, interner);
        }

        self.generation = Some(generation);
        self.palette_epoch = palette_epoch;
        self.viewport_top = viewport.top;
        self.scroll_offset = screen.scroll_offset();
        self.size = Some(screen.size());
        self.watermark = Watermark(seq);

        let update = if rebuild || self.changed.len() == self.rows.len() {
            SnapshotUpdate::Full
        } else if self.changed.is_empty() && scrolled == 0 && !meta_changed && !self.meta_dirty {
            SnapshotUpdate::Unchanged
        } else {
            SnapshotUpdate::Partial { scrolled }
        };
        if update != SnapshotUpdate::Unchanged {
            self.meta_dirty = false;
        }
        update
    }

    /// The signed distance the viewport top moved since the last copy (positive: down the
    /// output, so content moved up); `None` across the two screen lanes or past `i32`.
    fn viewport_delta(&self, top: RowId) -> Option<i32> {
        let old = self.viewport_top;
        if (old >= RowId::ALT_ORIGIN) != (top >= RowId::ALT_ORIGIN) {
            return None;
        }
        let distance = i32::try_from(top.0.abs_diff(old.0)).ok()?;
        Some(if top >= old { distance } else { -distance })
    }

    /// Copies every viewport row into freshly sized storage.
    fn rebuild(&mut self, screen: &Screen, viewport: Viewport, interner: &Interner) {
        self.rows
            .resize_with(usize::from(viewport.rows), SnapshotRow::default);
        self.hyperlinks.clear();
        self.force_full = false;
        for index in 0..viewport.rows {
            self.copy_row(screen, viewport.top, index, interner);
        }
    }

    /// Moves the cached rows with the content: a positive `scrolled` moves them up.
    /// `|scrolled| < rows.len()` is the caller's invariant; the shift clamps regardless.
    fn shift_rows(&mut self, scrolled: i32) {
        let by = usize::try_from(scrolled.unsigned_abs())
            .unwrap_or(usize::MAX)
            .min(self.rows.len());
        if scrolled > 0 {
            self.rows.rotate_left(by);
        } else {
            self.rows.rotate_right(by);
        }
    }

    /// Copies the viewport rows that moved in or were written since the watermark.
    fn copy_changed(&mut self, screen: &Screen, top: RowId, interner: &Interner) {
        for index in 0..screen.rows() {
            let id = top + u64::from(index);
            let stale = self
                .rows
                .get(usize::from(index))
                .is_none_or(|slot| slot.id != id || screen.row(id).seq() > self.watermark.0);
            if stale {
                self.copy_row(screen, top, index, interner);
            }
        }
    }

    fn copy_row(&mut self, screen: &Screen, top: RowId, index: u16, interner: &Interner) {
        let Some(slot) = self.rows.get_mut(usize::from(index)) else {
            return;
        };
        let row: RowRef<'_> = screen.row(top + u64::from(index));
        slot.copy_from(row, interner, &mut self.hyperlinks);
        self.changed.push(index);
        self.rows_copied = self.rows_copied.saturating_add(1);
    }
}

fn snapshot_cursor(screen: &Screen, visible: bool) -> SnapshotCursor {
    let pos = screen.cursor().pos;
    let row = pos
        .row
        .0
        .checked_sub(screen.visible_top().0)
        .and_then(|distance| u16::try_from(distance).ok())
        .filter(|row| *row < screen.rows());
    SnapshotCursor {
        id: pos.row,
        col: pos.col,
        row,
        visible,
    }
}

#[cfg(test)]
impl SnapshotState {
    /// The capacities of the `changed` list and the hyperlink cache, for the steadiness test.
    pub(super) fn buffer_capacities(&self) -> [usize; 2] {
        [self.changed.capacity(), self.hyperlinks.capacity()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphic_offset_is_relative_to_the_placement() {
        let mut state = SnapshotState::new();
        state.placements.push(SnapshotPlacement {
            id: GraphicId(7),
            row: RowId(10),
            col: 4,
            cols: 3,
            rows: 2,
            pixel_size: (30, 20),
        });
        let offset = |row: u64, col: u16| state.graphic_offset(GraphicId(7), RowId(row), col);
        assert_eq!(offset(10, 4), Some((0, 0)));
        assert_eq!(offset(11, 6), Some((2, 1)));
        assert_eq!(offset(9, 4), None);
        assert_eq!(offset(10, 3), None);
        assert_eq!(offset(12, 4), None);
        assert_eq!(offset(10, 7), None);
        assert_eq!(state.graphic_offset(GraphicId(8), RowId(10), 4), None);
    }
}
