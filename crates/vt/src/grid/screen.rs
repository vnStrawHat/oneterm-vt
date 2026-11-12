//! One screen: a ring of rows, a viewport, a cursor, a scroll region and the scroll primitives.
//!
//! The **screen** is the bottom `rows` rows and every VT operation addresses it by index;
//! the **viewport** is what the user sees, `offset` rows above the screen. Nothing here
//! errors or panics: counts clamp and out-of-range reads are blanks.
//!
//! The ring has a fixed number of slots (a power of two, sized once from the scrollback
//! limit); an id lives in slot `id & mask`. A slot is one pointer: rows are boxed, so a
//! region scroll rotates 8 bytes per row. Rows are created lazily: a slot that was never
//! written holds `None` and reads as blanks.

use std::fmt;
use std::ops::Range;

use rustc_hash::FxHashSet;

use crate::cell::{Cell, CellContent, Style};
use crate::grid::anchor::{AnchorId, AnchorKind, Anchors};
use crate::grid::integrity::IntegrityViolation;
use crate::grid::row::{Row, RowFlags, RowMut, RowRef, SeqNo};
use crate::grid::{MAX_ROWS, Pos, RowId, SCROLLBACK_MAX, ScrollRegion, Size, Viewport};
use crate::intern::{GraphemeArena, GraphemeId, GraphemeRemap, Interner, StyleId};

mod erase;
mod integrity;
mod print;
mod reflow;
mod resize;

pub use erase::{DisplayClear, LineClear};
pub use print::PrintMode;

/// Which of the two screens of a terminal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ScreenKind {
    /// The main screen, with scrollback.
    Primary,
    /// The alternate screen, without scrollback.
    Alternate,
}

impl ScreenKind {
    /// The first row id of the screen's lane.
    pub fn origin(self) -> RowId {
        match self {
            ScreenKind::Primary => RowId::PRIMARY_ORIGIN,
            ScreenKind::Alternate => RowId::ALT_ORIGIN,
        }
    }
}

/// A character set designated into a slot (`ESC (`, `ESC )`, ...).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Charset {
    /// US ASCII (`ESC ( B`).
    #[default]
    Ascii,
    /// DEC special characters and line drawing (`ESC ( 0`).
    SpecialCharacterAndLineDrawing,
}

/// What `CUP` is relative to (DECOM).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CursorOrigin {
    /// The screen's top-left (DECOM reset).
    #[default]
    Screen,
    /// The scroll region's top-left (DECOM set).
    Region,
}

/// The cursor and its pen.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cursor {
    /// Where the cursor is. The row is always on the screen.
    pub pos: Pos,
    /// The next printable character wraps first (the cursor sits after the last column).
    pub pending_wrap: bool,
    /// The four designated character sets.
    pub charsets: [Charset; 4],
    /// The cell a printed glyph inherits.
    template: Cell,
    /// What erases fill with: a blank carrying only the pen's background.
    erase: Cell,
}

impl Cursor {
    fn home(row: RowId) -> Cursor {
        Cursor {
            pos: Pos { row, col: 0 },
            pending_wrap: false,
            charsets: [Charset::Ascii; 4],
            template: Cell::EMPTY,
            erase: Cell::EMPTY,
        }
    }

    /// The cell erases and blank rows are filled with.
    pub fn erase(&self) -> Cell {
        self.erase
    }

    /// The cell a printed glyph inherits: the pen's style, extras and semantic zone.
    pub fn template(&self) -> Cell {
        self.template
    }
}

/// Rows whose content moved by `delta` ids, as inclusive ids after the move.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RowsScrolled {
    /// The first affected id.
    pub top: RowId,
    /// The last affected id.
    pub bottom: RowId,
    /// How many ids the content moved: positive towards newer ids.
    pub delta: i32,
}

/// What a scroll or clear did, for consumers that track rows.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScrollReport {
    /// Content that changed id; `None` when no row moved.
    pub scrolled: Option<RowsScrolled>,
    /// The new oldest id, when rows left the ring.
    pub trimmed: Option<RowId>,
    /// Rows that entered the scrollback.
    pub history_rows: u16,
}

impl ScrollReport {
    pub(crate) const NONE: ScrollReport = ScrollReport {
        scrolled: None,
        trimmed: None,
        history_rows: 0,
    };
}

/// Horizontal tab stops, one flag per column.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TabStops {
    stops: Vec<bool>,
}

impl TabStops {
    /// Creates stops every 8 columns for a screen `cols` wide.
    pub fn new(cols: u16) -> TabStops {
        TabStops {
            stops: (0..cols).map(|col| col % 8 == 0).collect(),
        }
    }

    /// Clears the stop at `col`.
    pub fn clear(&mut self, col: u16) {
        if let Some(stop) = self.stops.get_mut(usize::from(col)) {
            *stop = false;
        }
    }

    pub(crate) fn is_stop(&self, col: u16) -> bool {
        self.stops.get(usize::from(col)).copied().unwrap_or(false)
    }

    pub(crate) fn set(&mut self, col: u16) {
        if let Some(stop) = self.stops.get_mut(usize::from(col)) {
            *stop = true;
        }
    }

    pub(crate) fn clear_all(&mut self) {
        self.stops.fill(false);
    }

    /// Resizes to `cols` columns: the kept prefix is untouched, new columns get the default
    /// stop at every 8th absolute column.
    pub(crate) fn resize(&mut self, cols: u16) {
        let old = self.stops.len();
        let new = usize::from(cols);
        self.stops.truncate(new);
        self.stops.extend((old..new).map(|col| col % 8 == 0));
    }

    /// The first stop right of `col`, or the last column.
    pub(crate) fn next_after(&self, col: u16) -> u16 {
        let last = self.stops.len().saturating_sub(1);
        let start = usize::from(col) + 1;
        let next = self
            .stops
            .get(start..)
            .and_then(|rest| rest.iter().position(|&stop| stop))
            .map_or(last, |offset| start + offset);
        // `next < cols <= MAX_COLS`.
        next.min(last) as u16
    }

    /// The last stop left of `col`, or column 0.
    pub(crate) fn prev_before(&self, col: u16) -> u16 {
        let end = usize::from(col).min(self.stops.len());
        let prev = self
            .stops
            .get(..end)
            .and_then(|head| head.iter().rposition(|&stop| stop))
            .unwrap_or(0);
        // `prev < cols <= MAX_COLS`.
        prev as u16
    }
}

/// How a rotation moves its rows.
#[derive(Clone, Copy)]
enum Shift {
    /// Towards older ids.
    Up,
    /// Towards newer ids.
    Down,
}

/// One screen with its scrollback.
pub struct Screen {
    kind: ScreenKind,
    slots: Box<[Option<Box<Row>>]>,
    mask: u64,
    oldest: RowId,
    newest: RowId,
    /// The highest id this screen has ever had live; a resize may lower `newest` below it.
    newest_issued: RowId,
    rows: u16,
    cols: u16,
    scrollback_limit: u32,
    offset: u32,
    region: ScrollRegion,
    /// The left and right margins (DECSLRM) as columns, `right` exclusive: the whole width
    /// unless a program narrowed them under `?69`.
    left: u16,
    right: u16,
    /// Whether the margins are narrower than the screen, kept beside them so the print and
    /// line-feed paths test one flag.
    narrowed: bool,
    cursor: Cursor,
    saved_cursor: Cursor,
    tabs: TabStops,
    seq: SeqNo,
    /// The first id on the screen when the current batch opened: no older row can have
    /// been written in the batch.
    batch_lo: RowId,
    /// The last row a trim dropped, kept for its allocation; never read as content.
    spare: Option<Box<Row>>,
    /// Ids whose rows a rotation moved in this batch without rewriting their headers: the slot
    /// of a live id in this range holds that id's row (or nothing), whatever id its header
    /// says, changed at `seq`. [`restamp_moved`](Self::restamp_moved) rewrites the headers.
    /// Always among the live ids: trimming drops the ids it passes.
    moved: Range<RowId>,
    dropped_wide: u32,
    cursor_anchor: AnchorId,
    saved_cursor_anchor: AnchorId,
    viewport_anchor: AnchorId,
}

impl fmt::Debug for Screen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Screen")
            .field("kind", &self.kind)
            .field("rows", &self.rows)
            .field("cols", &self.cols)
            .field("oldest", &self.oldest)
            .field("newest", &self.newest)
            .field("offset", &self.offset)
            .field("cursor", &self.cursor.pos)
            .finish_non_exhaustive()
    }
}

impl Screen {
    /// Creates a screen of `size` (clamped) keeping up to `scrollback_limit` history rows
    /// (clamped to [`SCROLLBACK_MAX`]), and registers its three anchors.
    pub fn new(
        kind: ScreenKind,
        size: Size,
        scrollback_limit: u32,
        anchors: &mut Anchors,
    ) -> Screen {
        let size = size.clamped();
        let scrollback_limit = scrollback_limit.min(SCROLLBACK_MAX);
        // At most `1_000_000 + 1024` slots.
        let ring = (scrollback_limit as usize + usize::from(MAX_ROWS)).next_power_of_two();
        let slots: Box<[Option<Box<Row>>]> = std::iter::repeat_with(|| None).take(ring).collect();
        let oldest = kind.origin();
        let newest = oldest + u64::from(size.rows - 1);
        let home = Cursor::home(oldest);
        let origin_pos = home.pos;
        Screen {
            kind,
            slots,
            mask: (ring - 1) as u64,
            oldest,
            newest,
            newest_issued: newest,
            rows: size.rows,
            cols: size.cols,
            scrollback_limit,
            offset: 0,
            region: ScrollRegion::full(size.rows),
            left: 0,
            right: size.cols,
            narrowed: false,
            cursor: home,
            saved_cursor: home,
            tabs: TabStops::new(size.cols),
            seq: SeqNo(1),
            batch_lo: oldest,
            spare: None,
            moved: oldest..oldest,
            dropped_wide: 0,
            cursor_anchor: anchors.register(AnchorKind::Cursor, origin_pos),
            saved_cursor_anchor: anchors.register(AnchorKind::SavedCursor, origin_pos),
            viewport_anchor: anchors.register(AnchorKind::ViewportTop, origin_pos),
        }
    }

    /// Which screen this is.
    pub fn kind(&self) -> ScreenKind {
        self.kind
    }

    /// Screen rows.
    pub fn rows(&self) -> u16 {
        self.rows
    }

    /// Screen columns.
    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Screen size.
    pub fn size(&self) -> Size {
        Size {
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// The oldest live row id.
    pub fn oldest(&self) -> RowId {
        self.oldest
    }

    /// The newest live row id: the bottom screen row.
    pub fn newest(&self) -> RowId {
        self.newest
    }

    /// The live ids, `oldest..newest + 1`.
    pub fn row_range(&self) -> Range<RowId> {
        self.oldest..self.newest + 1
    }

    /// Live rows above the screen.
    pub fn history_len(&self) -> u32 {
        let live = self.newest.distance(self.oldest).saturating_add(1);
        let history = live.saturating_sub(u64::from(self.rows));
        u32::try_from(history).unwrap_or(u32::MAX)
    }

    /// The id of the top screen row.
    pub fn screen_top(&self) -> RowId {
        self.newest - u64::from(self.rows - 1)
    }

    /// The id of the top viewport row.
    pub fn visible_top(&self) -> RowId {
        self.screen_top() - u64::from(self.offset)
    }

    /// How many rows the viewport is scrolled back; 0 follows output.
    pub fn scroll_offset(&self) -> u32 {
        self.offset
    }

    /// Scrolls the viewport; negative moves towards history. Clamps to the history.
    pub fn scroll_viewport(&mut self, delta: i32) {
        let target = i64::from(self.offset) - i64::from(delta);
        let clamped = target.clamp(0, i64::from(self.history_len()));
        self.offset = u32::try_from(clamped).unwrap_or(0);
        self.debug_check();
    }

    /// Makes the viewport follow output again.
    pub fn scroll_to_bottom(&mut self) {
        self.offset = 0;
        self.debug_check();
    }

    fn slot_index(&self, id: RowId) -> usize {
        // The mask is below `2^21`, so it fits `usize`.
        (id.0 & self.mask) as usize
    }

    /// The row stored for `id`, if its slot holds exactly that id (or a row a rotation moved
    /// there, whose header is not rewritten yet).
    fn stored(&self, id: RowId) -> Option<&Row> {
        self.slots
            .get(self.slot_index(id))?
            .as_deref()
            .filter(|row| row.id() == id || self.moved.contains(&id))
    }

    /// Rewrites the headers of the rows the rotations of this batch moved: their ids and the
    /// batch stamp. Rows outside the live ids are left alone (their slots may hold other ids).
    pub(crate) fn restamp_moved(&mut self) {
        let lo = self.moved.start.max(self.oldest);
        let hi = self.moved.end.min(self.newest + 1);
        let seq = self.seq;
        let mut id = lo;
        while id < hi {
            // A blank row a rotation moved (an empty slot) is built, so its stamp outlives the
            // batch: the content it replaced is gone.
            if self.materialise(id).is_some()
                && let Some(row) = self.slot_row_mut(id)
            {
                row.rehome(id, seq);
            }
            id = id + 1;
        }
        self.moved = self.oldest..self.oldest;
    }

    /// The row in the slot of `id`, whatever id it carries.
    fn slot_row_mut(&mut self, id: RowId) -> Option<&mut Row> {
        let index = self.slot_index(id);
        self.slots.get_mut(index)?.as_deref_mut()
    }

    /// Reads a row: the stored row, or blanks if the id's slot holds no such row.
    pub fn row(&self, id: RowId) -> RowRef<'_> {
        match self.stored(id) {
            Some(row) if self.moved.contains(&id) => RowRef::moved(row, id, self.seq),
            Some(row) => RowRef::allocated(row),
            None if self.moved.contains(&id) => RowRef::blank(id, self.cols).moved_at(self.seq),
            None => RowRef::blank(id, self.cols),
        }
    }

    /// Opens a live row for writing, creating it blank if it was never written. `None` for an
    /// id outside the live range (its slot may belong to a live row).
    pub fn row_mut(&mut self, id: RowId) -> Option<RowMut<'_>> {
        if id < self.oldest || id > self.newest {
            return None;
        }
        let seq = self.seq;
        self.materialise(id).map(|row| RowMut::new(row, seq))
    }

    /// Makes the slot of live `id` hold its row, built blank from the spare allocation.
    fn materialise(&mut self, id: RowId) -> Option<&mut Row> {
        let index = self.slot_index(id);
        let missing = match self.slots.get(index)? {
            Some(row) => row.id() != id && !self.moved.contains(&id),
            None => true,
        };
        // ponytail: a slot holding a different id is unreachable today (trim and clear_history
        // take the slot first, push_row overwrites); if it were reached this would allocate.
        if missing {
            let row = Row::recycle(self.spare.take(), id, self.cols, self.seq, Cell::EMPTY);
            *self.slots.get_mut(index)? = Some(row);
        }
        self.slots.get_mut(index)?.as_deref_mut()
    }

    /// The id of screen row `index`; clamps to the screen.
    pub fn row_of_index(&self, index: u16) -> RowId {
        self.screen_top() + u64::from(index.min(self.rows - 1))
    }

    /// The screen index of `id`, or `None` when it is not a screen row.
    pub(crate) fn index_of(&self, id: RowId) -> Option<u16> {
        if id < self.screen_top() || id > self.newest {
            return None;
        }
        u16::try_from(id.distance(self.screen_top())).ok()
    }

    /// The cursor.
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// The cursor saved by DECSC.
    pub fn saved_cursor(&self) -> &Cursor {
        &self.saved_cursor
    }

    /// The screen index of the cursor row.
    pub fn cursor_row_index(&self) -> u16 {
        self.index_of(self.cursor.pos.row).unwrap_or(self.rows - 1)
    }

    /// The anchor that follows the cursor.
    pub fn cursor_anchor(&self) -> AnchorId {
        self.cursor_anchor
    }

    pub(crate) fn region(&self) -> ScrollRegion {
        self.region
    }

    pub(crate) fn tabs(&self) -> &TabStops {
        &self.tabs
    }

    pub(crate) fn tabs_mut(&mut self) -> &mut TabStops {
        &mut self.tabs
    }

    /// Wide glyphs that could not be placed (one column, or the last column with autowrap off).
    pub fn dropped_wide(&self) -> u32 {
        self.dropped_wide
    }

    /// The cell and erase cell for glyphs and erases from now on: the pen.
    pub(crate) fn set_template(&mut self, template: Cell, interner: &mut Interner) {
        self.cursor.template = template;
        let bg = interner.resolve_style(template.style_id()).bg;
        // The erase cell carries only the pen's background, which rarely changes.
        if interner.resolve_style(self.cursor.erase.style_id()).bg == bg {
            return;
        }
        let bg_only = Style {
            bg,
            ..Style::DEFAULT
        };
        let erase_style = if bg == Style::DEFAULT.bg {
            StyleId::DEFAULT
        } else if *interner.resolve_style(template.style_id()) == bg_only {
            // The pen sets only a background: it is its own erase style, no second lookup.
            template.style_id()
        } else {
            interner.style(bg_only)
        };
        self.cursor.erase = Cell::EMPTY.with_style(erase_style);
    }

    /// Moves the cursor to a screen index and column, clamped; clears pending wrap.
    pub(crate) fn goto(&mut self, index: u16, col: u16) {
        self.cursor.pos = Pos {
            row: self.row_of_index(index),
            col: col.min(self.cols - 1),
        };
        self.cursor.pending_wrap = false;
    }

    /// `CUP`: moves the cursor to a screen index and column (clamped, pending wrap cleared),
    /// counted from the region top under `CursorOrigin::Region`.
    pub fn goto_origin(&mut self, index: u16, col: u16, origin: CursorOrigin) {
        let index = self.origin_row(index, origin);
        let col = self.origin_col(col, origin);
        self.goto(index, col);
        self.debug_check();
    }

    /// `VPA`: moves to a screen row, counted from the region top under
    /// `CursorOrigin::Region`; the column stays.
    pub(crate) fn goto_row(&mut self, index: u16, origin: CursorOrigin) {
        let index = self.origin_row(index, origin);
        let col = self.cursor.pos.col;
        self.goto(index, col);
        self.debug_check();
    }

    /// `CHA` and `HPA`: moves to a column of the cursor row, counted from the left margin
    /// under `CursorOrigin::Region`.
    pub(crate) fn goto_column(&mut self, col: u16, origin: CursorOrigin) {
        let index = self.cursor_row_index();
        let col = self.origin_col(col, origin);
        self.goto(index, col);
    }

    fn origin_row(&self, index: u16, origin: CursorOrigin) -> u16 {
        match origin {
            CursorOrigin::Screen => index,
            CursorOrigin::Region => index
                .saturating_add(self.region.top)
                .min(self.region.bottom.saturating_sub(1)),
        }
    }

    fn origin_col(&self, col: u16, origin: CursorOrigin) -> u16 {
        match origin {
            CursorOrigin::Screen => col,
            CursorOrigin::Region => col
                .saturating_add(self.left)
                .min(self.right.saturating_sub(1)),
        }
    }

    /// The side margins as `(left, right)` columns, `right` exclusive: `(0, cols)` unless a
    /// program set them with DECSLRM.
    pub fn side_margins(&self) -> (u16, u16) {
        (self.left, self.right)
    }

    /// Whether DECSLRM narrowed the margins. The bulk print and scroll paths assume it did
    /// not; the general ones honour them.
    #[inline]
    pub(crate) fn has_side_margins(&self) -> bool {
        self.narrowed
    }

    /// `DECSLRM` with 0-based columns, `right` exclusive. Accepted when at least two columns
    /// remain after clamping `right` to the width; on success the cursor homes.
    pub(crate) fn set_side_margins(&mut self, left: u16, right: u16, origin: CursorOrigin) -> bool {
        let right = right.min(self.cols);
        if left.saturating_add(1) >= right {
            return false;
        }
        self.left = left;
        self.right = right;
        self.narrowed = left > 0 || right < self.cols;
        self.goto_origin(0, 0, origin);
        true
    }

    /// Back to the whole width.
    pub(crate) fn reset_side_margins(&mut self) {
        self.left = 0;
        self.right = self.cols;
        self.narrowed = false;
    }

    /// Whether the cursor is between the side margins.
    pub(crate) fn inside_side_margins(&self) -> bool {
        (self.left..self.right).contains(&self.cursor.pos.col)
    }

    /// One past the last column a glyph at the cursor may take: the right margin while the
    /// cursor is left of it, else the screen edge.
    pub(crate) fn right_limit(&self) -> u16 {
        if self.cursor.pos.col < self.right {
            self.right
        } else {
            self.cols
        }
    }

    /// The column the cursor does not move left of: the left margin unless it is already
    /// left of it.
    pub(crate) fn left_limit(&self) -> u16 {
        if self.cursor.pos.col >= self.left {
            self.left
        } else {
            0
        }
    }

    /// `CUF`: right `n` columns, stopping at the right margin (or the screen edge beyond it).
    pub(crate) fn cursor_forward(&mut self, n: u16) {
        let index = self.cursor_row_index();
        let col = self.cursor.pos.col.saturating_add(n);
        self.goto(index, col.min(self.right_limit() - 1));
    }

    /// `CUB`: left `n` columns, stopping at the left margin (or column 0 left of it).
    pub(crate) fn cursor_back(&mut self, n: u16) {
        let index = self.cursor_row_index();
        let col = self.cursor.pos.col.saturating_sub(n);
        self.goto(index, col.max(self.left_limit()));
    }

    /// `DECSTBM` with 0-based, bottom-exclusive rows. Returns whether the region was accepted
    /// (`top < bottom` after clamping `bottom` to the screen); on success the cursor homes.
    pub fn set_region(&mut self, top: u16, bottom: u16, origin: CursorOrigin) -> bool {
        let bottom = bottom.min(self.rows);
        if top >= bottom {
            return false;
        }
        self.region = ScrollRegion { top, bottom };
        self.goto_origin(0, 0, origin);
        true
    }

    pub(crate) fn carriage_return(&mut self) {
        // Column 0 without a look at the margins unless a program narrowed them.
        self.cursor.pos.col = if self.narrowed { self.left_limit() } else { 0 };
        self.cursor.pending_wrap = false;
    }

    /// `BS`: one column left, pending wrap cleared. At column 0 it does nothing (pending wrap
    /// included), except under reverse wrap: below the region top, when the row above wrapped
    /// into this one, it moves to that row's last column.
    pub(crate) fn backspace(&mut self, mode: PrintMode) {
        if self.cursor.pos.col > self.left_limit() {
            self.cursor.pos.col -= 1;
            self.cursor.pending_wrap = false;
            return;
        }
        // Rows cut short by a right margin are not marked as wrapped: nothing to go back to.
        let at_top = self.cursor_row_index() <= self.region.top;
        if !mode.reverse_wrap || self.has_side_margins() || at_top {
            return;
        }
        let above = self.cursor.pos.row - 1;
        if self.row(above).wrapped() {
            self.cursor.pos.row = above;
            self.cursor.pos.col = self.cols - 1;
            self.cursor.pending_wrap = false;
        }
    }

    /// `ESC (`, `)`, `*`, `+`: designates `charset` into `slot`; a slot above 3 is ignored.
    pub(crate) fn designate_charset(&mut self, slot: u8, charset: Charset) {
        if let Some(entry) = self.cursor.charsets.get_mut(usize::from(slot)) {
            *entry = charset;
        }
    }

    pub(crate) fn save_cursor(&mut self) {
        self.saved_cursor = self.cursor;
    }

    pub(crate) fn restore_cursor(&mut self) {
        self.cursor = self.saved_cursor;
        self.cursor.pos.row = self.row_of_index(self.cursor_row_index());
        self.cursor.pos.col = self.cursor.pos.col.min(self.cols - 1);
        self.debug_check();
    }

    /// The viewport as a snapshot sees it.
    pub(crate) fn viewport(&self) -> Viewport {
        Viewport {
            top: self.visible_top(),
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// Opens a batch: later changes carry `seq`.
    pub(crate) fn set_seq(&mut self, seq: SeqNo) {
        self.restamp_moved();
        self.seq = seq;
        self.batch_lo = self.screen_top();
    }

    /// Writes the screen-owned anchors back from the screen's fields.
    pub(crate) fn sync_anchors(&self, anchors: &mut Anchors) {
        for (id, pos) in self.owned_anchors() {
            anchors.set(id, pos);
        }
    }

    /// The screen-owned anchors with the positions the fields say they hold.
    pub(crate) fn owned_anchors(&self) -> [(AnchorId, Pos); 3] {
        let top = Pos {
            row: self.visible_top(),
            col: 0,
        };
        [
            (self.cursor_anchor, self.cursor.pos),
            (self.saved_cursor_anchor, self.saved_cursor.pos),
            (self.viewport_anchor, top),
        ]
    }

    /// The region and tab stops are shared by VT semantics but owned per screen; this copies
    /// them from the other screen.
    pub(crate) fn adopt_region_and_tabs(&mut self, from: &Screen) {
        self.region = from.region;
        self.tabs = from.tabs.clone();
        (self.left, self.right) = (from.left, from.right.min(self.cols));
        self.narrowed = self.left > 0 || self.right < self.cols;
        if self.left.saturating_add(1) >= self.right {
            self.reset_side_margins();
        }
    }

    /// Takes the other screen's cursor (pens, charsets, pending wrap) at the same screen index.
    pub(crate) fn adopt_cursor(&mut self, from: &Screen) {
        let mut cursor = from.cursor;
        cursor.pos.row = self.row_of_index(from.cursor_row_index());
        cursor.pos.col = cursor.pos.col.min(self.cols - 1);
        self.cursor = cursor;
        self.debug_check();
    }

    /// Resets the rows of the id `range` to the erase cell. A row never written stays unwritten
    /// when the erase cell is the plain blank.
    fn reset_rows(&mut self, ids: Range<RowId>) {
        let mut id = ids.start;
        while id < ids.end {
            self.reset_row(id);
            id = id + 1;
        }
    }

    fn reset_row(&mut self, id: RowId) {
        let erase = self.cursor.erase;
        if erase == Cell::EMPTY && !self.row(id).is_allocated() {
            return;
        }
        let seq = self.seq;
        if let Some(mut row) = self.row_mut(id) {
            row.reset(erase, seq);
        }
    }

    /// Resets screen rows `first..end` by index.
    pub(crate) fn reset_screen_rows(&mut self, first: u16, end: u16) {
        let top = self.screen_top();
        self.reset_rows(top + u64::from(first)..top + u64::from(end.min(self.rows)));
    }

    /// Keeps `moved` among the live ids: a trimmed id's slot may already hold a newer row, which
    /// must not read as the trimmed id's moved row.
    fn forget_moved_below_oldest(&mut self) {
        if self.moved.start < self.oldest {
            self.moved.start = self.oldest;
        }
    }

    /// Drops the oldest ids over the scrollback limit; returns how many.
    fn trim_history(&mut self) -> u32 {
        let mut trimmed = 0u32;
        while self.history_len() > self.scrollback_limit {
            let index = self.slot_index(self.oldest);
            if let Some(row) = self.slots.get_mut(index).and_then(Option::take) {
                self.spare = Some(row);
            }
            self.oldest = self.oldest + 1;
            trimmed = trimmed.saturating_add(1);
        }
        if trimmed > 0 {
            self.forget_moved_below_oldest();
        }
        trimmed
    }

    /// Adds one id below the screen filled with `erase`, trimming the oldest ids over the
    /// scrollback limit; returns how many it trimmed.
    fn push_row(&mut self, erase: Cell) -> u32 {
        self.newest = self.newest + 1;
        let reissued = self.newest <= self.newest_issued;
        self.newest_issued = self.newest_issued.max(self.newest);
        let trimmed = self.trim_history();
        // A plain blank row needs no stamp: nobody has seen the new id. A released id was seen,
        // so it comes back stamped.
        let row = (erase != Cell::EMPTY || reissued)
            .then(|| Row::recycle(self.spare.take(), self.newest, self.cols, self.seq, erase));
        let index = self.slot_index(self.newest);
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = row;
        }
        trimmed
    }

    fn swap_slots(&mut self, a: RowId, b: RowId) {
        let (a, b) = (self.slot_index(a), self.slot_index(b));
        if a != b && a < self.slots.len() && b < self.slots.len() {
            self.slots.swap(a, b);
        }
    }

    /// Moves the content of the `len` rows from `lo` by `n` ids and blanks the `n` vacated
    /// rows. Rows move between slots, so nothing is allocated unless a slot in the range was
    /// never written.
    ///
    /// A window that does not wrap the ring end rotates as one slice; one that does (at most
    /// once per ring cycle) falls back to [`rotate_by_swaps`](Self::rotate_by_swaps).
    fn rotate(&mut self, lo: RowId, len: u64, shift: Shift, n: u64) {
        let start = self.slot_index(lo);
        let Ok(count) = usize::try_from(len) else {
            return self.rotate_by_swaps(lo, len, shift, n);
        };
        let end = start.saturating_add(count);
        if end > self.slots.len() {
            return self.rotate_by_swaps(lo, len, shift, n);
        }
        // A window apart from the ones already moved would widen `moved` over the rows between
        // them, which then read as changed: settle the earlier ones first.
        let hi = lo + len;
        if !self.moved.is_empty() && (lo > self.moved.end || hi < self.moved.start) {
            self.restamp_moved();
        }
        let (seq, erase, cols) = (self.seq, self.cursor.erase, self.cols);
        let Screen { slots, spare, .. } = self;
        let Some(window) = slots.get_mut(start..end) else {
            return;
        };
        let shifted = usize::try_from(n).map_or(count, |n| n.min(count));
        let vacated = match shift {
            Shift::Up => {
                window.rotate_left(shifted);
                count - shifted..count
            }
            Shift::Down => {
                window.rotate_right(shifted);
                0..shifted
            }
        };
        // Only the vacated rows are written; the moved ones keep their old headers until
        // `restamp_moved` (an empty slot is a blank row wherever it lands).
        let start_k = vacated.start;
        let taken = vacated.len();
        for (k, slot) in window.iter_mut().enumerate().skip(start_k).take(taken) {
            let id = lo + k as u64;
            match slot {
                Some(row) => {
                    row.rehome(id, seq);
                    row.reset(erase, seq);
                }
                None if erase != Cell::EMPTY => {
                    *slot = Some(Row::recycle(spare.take(), id, cols, seq, erase));
                }
                None => {}
            }
        }
        self.moved = if self.moved.is_empty() {
            lo..hi
        } else {
            self.moved.start.min(lo)..self.moved.end.max(hi)
        };
    }

    /// The swap-by-swap form of [`rotate`](Self::rotate), for a window that wraps the ring end.
    fn rotate_by_swaps(&mut self, lo: RowId, len: u64, shift: Shift, n: u64) {
        self.restamp_moved();
        for k in 0..len {
            self.materialise(lo + k);
        }
        let (moved, vacated) = match shift {
            Shift::Up => {
                for k in 0..len.saturating_sub(n) {
                    self.swap_slots(lo + k, lo + k + n);
                }
                (0..len.saturating_sub(n), len.saturating_sub(n)..len)
            }
            Shift::Down => {
                for k in (n..len).rev() {
                    self.swap_slots(lo + k, lo + k - n);
                }
                (n..len, 0..n.min(len))
            }
        };
        let (seq, erase) = (self.seq, self.cursor.erase);
        for k in moved {
            if let Some(row) = self.slot_row_mut(lo + k) {
                row.rehome(lo + k, seq);
            }
        }
        for k in vacated {
            if let Some(row) = self.slot_row_mut(lo + k) {
                row.rehome(lo + k, seq);
                row.reset(erase, seq);
            }
        }
    }

    /// Clamps a region and count to the screen; `None` when nothing would move.
    fn clamp_scroll(&self, region: ScrollRegion, n: u16) -> Option<(ScrollRegion, u16)> {
        let bottom = region.bottom.min(self.rows);
        let region = ScrollRegion {
            top: region.top.min(bottom),
            bottom,
        };
        let n = n.min(region.height());
        (n > 0).then_some((region, n))
    }

    /// Scrolls `region` up by `n` rows, blanking the rows opened at its bottom.
    ///
    /// A region that starts at the top of the primary screen, or covers the whole alternate
    /// screen, feeds the scrolled-out rows to the history; rows below such a region keep their
    /// screen index. Any other region drops them and rotates in place; the alternate screen still
    /// counts the rows leaving its top as history rows, which it drops at once.
    #[cfg_attr(feature = "hotpath-profiling", hotpath::measure(impl_type = "Screen"))]
    pub(crate) fn scroll_up(
        &mut self,
        region: ScrollRegion,
        n: u16,
        anchors: &mut Anchors,
    ) -> ScrollReport {
        let Some((region, n)) = self.clamp_scroll(region, n) else {
            return ScrollReport::NONE;
        };
        if self.has_side_margins() {
            return self.scroll_rect(region, n, Shift::Up, anchors);
        }
        let in_place = self.kind == ScreenKind::Alternate && region.bottom < self.rows;
        let report = if region.top == 0 && !in_place {
            self.scroll_up_into_history(region, n, anchors)
        } else {
            let lo = self.screen_top() + u64::from(region.top);
            let len = u64::from(region.height());
            self.rotate(lo, len, Shift::Up, u64::from(n));
            let step = u64::from(n);
            anchors.shift_region(lo..lo + len, -i32::from(n), lo..lo + step);
            ScrollReport {
                scrolled: Some(RowsScrolled {
                    top: lo,
                    bottom: lo + (len - 1),
                    delta: -i32::from(n),
                }),
                // The alternate screen's top rows still leave the top of the screen.
                history_rows: if region.top == 0 { n } else { 0 },
                ..ScrollReport::NONE
            }
        };
        self.sync_anchors(anchors);
        self.debug_check();
        report
    }

    fn scroll_up_into_history(
        &mut self,
        region: ScrollRegion,
        n: u16,
        anchors: &mut Anchors,
    ) -> ScrollReport {
        let step = u64::from(n);
        let old_top = self.screen_top();
        let old_newest = self.newest;
        let oldest_before = self.oldest;
        for _ in 0..n {
            self.push_row(self.cursor.erase);
        }
        // The cursors keep their screen index; the content below the region keeps it too.
        self.cursor.pos.row = self.cursor.pos.row + step;
        self.saved_cursor.pos.row = self.saved_cursor.pos.row + step;
        if self.offset != 0 {
            self.offset = self
                .offset
                .saturating_add(u32::from(n))
                .min(self.history_len());
        }
        let mut scrolled = None;
        if region.bottom < self.rows {
            let tail_top = old_top + u64::from(region.bottom);
            let len = u64::from(self.rows - region.bottom) + step;
            self.rotate(tail_top, len, Shift::Down, step);
            anchors.shift_region(
                tail_top..old_newest + 1,
                i32::from(n),
                RowId::default()..RowId::default(),
            );
            scrolled = Some(RowsScrolled {
                top: tail_top + step,
                bottom: self.newest,
                delta: i32::from(n),
            });
        }
        let trimmed = (self.oldest > oldest_before).then_some(self.oldest);
        if trimmed.is_some() {
            anchors.trim(self.kind.origin(), self.oldest);
        }
        ScrollReport {
            scrolled,
            trimmed,
            history_rows: n,
        }
    }

    /// Scrolls `region` down by `n` rows, blanking the rows opened at its top. Never pulls
    /// from the history.
    #[cfg_attr(feature = "hotpath-profiling", hotpath::measure(impl_type = "Screen"))]
    pub(crate) fn scroll_down(
        &mut self,
        region: ScrollRegion,
        n: u16,
        anchors: &mut Anchors,
    ) -> ScrollReport {
        let Some((region, n)) = self.clamp_scroll(region, n) else {
            return ScrollReport::NONE;
        };
        if self.has_side_margins() {
            return self.scroll_rect(region, n, Shift::Down, anchors);
        }
        let lo = self.screen_top() + u64::from(region.top);
        let len = u64::from(region.height());
        let step = u64::from(n);
        self.rotate(lo, len, Shift::Down, step);
        anchors.shift_region(lo..lo + len, i32::from(n), lo + (len - step)..lo + len);
        self.sync_anchors(anchors);
        self.debug_check();
        ScrollReport {
            scrolled: Some(RowsScrolled {
                top: lo,
                bottom: lo + (len - 1),
                delta: i32::from(n),
            }),
            ..ScrollReport::NONE
        }
    }

    /// Scrolls the columns between the side margins of `region` by `n` rows, blanking the
    /// cells it opens. Cells move between rows, so nothing enters the history and no row
    /// changes its id; a position that followed a whole row has nothing left to follow, so
    /// the content anchors on these rows die.
    fn scroll_rect(
        &mut self,
        region: ScrollRegion,
        n: u16,
        shift: Shift,
        anchors: &mut Anchors,
    ) -> ScrollReport {
        let (left, right) = (self.left, self.right);
        let top = self.screen_top();
        let rows = top + u64::from(region.top)..top + u64::from(region.bottom);
        anchors.shift_region(rows.clone(), 0, rows);
        let erase = self.cursor.erase;
        // One short-lived buffer per scroll: a field for it made `Screen` larger and cost the
        // unmargined print path a few percent.
        let mut cells = Vec::with_capacity(usize::from(right - left));
        for step in 0..region.height() {
            // Destinations in the order that never reads a row already overwritten.
            let (dest, source) = match shift {
                Shift::Up => {
                    let dest = region.top + step;
                    (dest, dest.checked_add(n).filter(|row| *row < region.bottom))
                }
                Shift::Down => {
                    let dest = region.bottom - 1 - step;
                    (dest, dest.checked_sub(n).filter(|row| *row >= region.top))
                }
            };
            cells.clear();
            match source {
                Some(source) => {
                    let row = self.row(top + u64::from(source));
                    let range = usize::from(left)..usize::from(right);
                    cells.extend_from_slice(row.cells().get(range).unwrap_or_default());
                }
                None => cells.resize(usize::from(right - left), erase),
            }
            if let Some(mut row) = self.row_mut(top + u64::from(dest)) {
                row.write_cells(left, &cells);
            }
        }
        self.sync_anchors(anchors);
        self.debug_check();
        ScrollReport::NONE
    }

    /// `IND`/`LF`: down one row, scrolling the region up at its bottom. Returns the report of
    /// that scroll, `Some` exactly when one ran.
    ///
    /// Not for a screen with narrowed side margins: the caller takes
    /// [`linefeed_in_margins`](Self::linefeed_in_margins) while `?69` is set, so this path
    /// pays nothing for them (a flag test here cost `short_lines` 5 %).
    pub(crate) fn linefeed(&mut self, anchors: &mut Anchors) -> Option<ScrollReport> {
        let index = self.cursor_row_index();
        if index + 1 == self.region.bottom {
            if self.region.top == 0 && self.region.bottom == self.rows && self.offset == 0 {
                return Some(self.linefeed_into_history(anchors));
            }
            return Some(self.scroll_up(self.region, 1, anchors));
        }
        if index + 1 < self.rows {
            self.cursor.pos.row = self.cursor.pos.row + 1;
        }
        self.sync_anchors(anchors);
        self.debug_check();
        None
    }

    /// `IND`/`LF` while `?69` is set: at the region bottom only the columns between the side
    /// margins scroll, and only when the cursor is between them; outside, nothing moves.
    pub(crate) fn linefeed_in_margins(&mut self, anchors: &mut Anchors) -> Option<ScrollReport> {
        let index = self.cursor_row_index();
        if index + 1 == self.region.bottom {
            if !self.inside_side_margins() {
                return None;
            }
            return Some(self.scroll_up(self.region, 1, anchors));
        }
        if index + 1 < self.rows {
            self.cursor.pos.row = self.cursor.pos.row + 1;
        }
        self.sync_anchors(anchors);
        self.debug_check();
        None
    }

    /// `scroll_up_into_history` for one row of a whole-screen region and a live viewport.
    #[cfg_attr(feature = "hotpath-profiling", hotpath::measure(impl_type = "Screen"))]
    fn linefeed_into_history(&mut self, anchors: &mut Anchors) -> ScrollReport {
        let oldest_before = self.oldest;
        self.push_row(self.cursor.erase);
        self.cursor.pos.row = self.cursor.pos.row + 1;
        self.saved_cursor.pos.row = self.saved_cursor.pos.row + 1;
        let trimmed = (self.oldest > oldest_before).then_some(self.oldest);
        if trimmed.is_some() {
            anchors.trim(self.kind.origin(), self.oldest);
        }
        self.sync_anchors(anchors);
        self.debug_check();
        ScrollReport {
            scrolled: None,
            trimmed,
            history_rows: 1,
        }
    }

    /// `RI`: up one row, scrolling the region down at its top. Returns the report of that
    /// scroll, `Some` exactly when one ran.
    pub(crate) fn reverse_index(&mut self, anchors: &mut Anchors) -> Option<ScrollReport> {
        let index = self.cursor_row_index();
        if index == self.region.top {
            if self.has_side_margins() && !self.inside_side_margins() {
                return None;
            }
            return Some(self.scroll_down(self.region, 1, anchors));
        }
        if index > 0 {
            self.cursor.pos.row = self.cursor.pos.row - 1;
        }
        self.sync_anchors(anchors);
        self.debug_check();
        None
    }

    /// `IL`: opens `n` blank rows at the cursor row. `None` when the cursor is outside the region.
    pub(crate) fn insert_lines(&mut self, n: u16, anchors: &mut Anchors) -> Option<ScrollReport> {
        let index = self.cursor_row_index();
        if !self.region.contains(index) || (self.has_side_margins() && !self.inside_side_margins())
        {
            return None;
        }
        let region = ScrollRegion {
            top: index,
            bottom: self.region.bottom,
        };
        Some(self.scroll_down(region, n, anchors))
    }

    /// `DL`: deletes `n` rows at the cursor row. At screen row 0 they enter the history.
    pub(crate) fn delete_lines(&mut self, n: u16, anchors: &mut Anchors) -> Option<ScrollReport> {
        let index = self.cursor_row_index();
        if !self.region.contains(index) || (self.has_side_margins() && !self.inside_side_margins())
        {
            return None;
        }
        let region = ScrollRegion {
            top: index,
            bottom: self.region.bottom,
        };
        Some(self.scroll_up(region, n, anchors))
    }

    /// `ED 3`: drops the whole history. `trimmed` is set when there was any.
    pub(crate) fn clear_history(&mut self, anchors: &mut Anchors) -> ScrollReport {
        self.offset = 0;
        let top = self.screen_top();
        let had_history = top > self.oldest;
        let mut id = self.oldest;
        while id < top {
            let index = self.slot_index(id);
            if let Some(row) = self.slots.get_mut(index).and_then(Option::take) {
                self.spare = Some(row);
            }
            id = id + 1;
        }
        self.oldest = top;
        self.forget_moved_below_oldest();
        if had_history {
            anchors.trim(self.kind.origin(), top);
        }
        self.sync_anchors(anchors);
        self.debug_check();
        ScrollReport {
            trimmed: had_history.then_some(top),
            ..ScrollReport::NONE
        }
    }

    /// Replaces the content context (alternate screen entry): kills the anchors on the screen
    /// rows, follows output and blanks every screen row.
    pub(crate) fn wipe(&mut self, anchors: &mut Anchors) {
        let rows = self.screen_top()..self.newest + 1;
        anchors.shift_region(rows.clone(), 0, rows);
        self.offset = 0;
        self.reset_screen_rows(0, self.rows);
        self.sync_anchors(anchors);
        self.debug_check();
    }

    /// `RIS` for this screen: clears the history and the screen, homes both cursors with plain
    /// pens, restores the full region and the default tab stops. Ids are not reset.
    pub fn reset(&mut self, anchors: &mut Anchors) {
        self.restamp_moved();
        self.clear_history(anchors);
        let home = Cursor::home(self.screen_top());
        self.cursor = home;
        self.saved_cursor = home;
        self.region = ScrollRegion::full(self.rows);
        self.reset_side_margins();
        self.tabs = TabStops::new(self.cols);
        self.reset_screen_rows(0, self.rows);
        self.sync_anchors(anchors);
        self.debug_check();
    }

    /// Changes the scrollback limit (clamped to [`SCROLLBACK_MAX`]); the same value does nothing.
    ///
    /// The oldest rows over a lower limit are trimmed at once (their anchors die). When the
    /// ring size that fits the limit differs, the rows are moved to a ring of that size, once;
    /// ids never change.
    pub(crate) fn set_scrollback_limit(&mut self, limit: u32, anchors: &mut Anchors) {
        let limit = limit.min(SCROLLBACK_MAX);
        if limit == self.scrollback_limit {
            return;
        }
        self.restamp_moved();
        self.scrollback_limit = limit;
        if self.trim_history() > 0 {
            anchors.trim(self.kind.origin(), self.oldest);
        }
        self.offset = self.offset.min(self.history_len());
        // At most `1_000_000 + 1024` slots.
        let ring = (limit as usize + usize::from(MAX_ROWS)).next_power_of_two();
        if ring != self.slots.len() {
            let mask = (ring - 1) as u64;
            let mut slots: Box<[Option<Box<Row>>]> =
                std::iter::repeat_with(|| None).take(ring).collect();
            // A row outside the live ids is stale: it must not take the slot of a live one.
            let live = self.oldest..=self.newest;
            for row in self
                .slots
                .iter_mut()
                .filter_map(Option::take)
                .filter(|row| live.contains(&row.id()))
            {
                // The mask is below `2^21`, so it fits `usize`.
                if let Some(slot) = slots.get_mut((row.id().0 & mask) as usize) {
                    *slot = Some(row);
                }
            }
            self.slots = slots;
            self.mask = mask;
        }
        self.sync_anchors(anchors);
        self.debug_check();
    }

    /// The cluster ids held by the live rows flagged `HAS_GRAPHEME`, with repeats, newest row
    /// first: the order a sweep keeps clusters in.
    pub(crate) fn grapheme_ids(&self) -> impl Iterator<Item = GraphemeId> + '_ {
        let span = self.newest.distance(self.oldest);
        (0..=span)
            .filter_map(move |back| self.stored(self.newest - back))
            .filter(|row| row.header().flags.contains(RowFlags::HAS_GRAPHEME))
            .flat_map(|row| {
                row.cells().iter().filter_map(|cell| match cell.content() {
                    CellContent::Grapheme(id) => Some(id),
                    CellContent::Scalar(_) => None,
                })
            })
    }

    /// Adds to `out` the style ids this screen holds: every cell of the rows flagged `STYLED`
    /// (history included; the hint has no false negatives) and the pen and erase cell of both
    /// cursors.
    pub(crate) fn collect_style_ids(&self, out: &mut FxHashSet<u16>) {
        for row in self
            .slots
            .iter()
            .flatten()
            .filter(|row| row.header().flags.contains(RowFlags::STYLED))
        {
            // Cells from `occ` on equal the last one, so the prefix and the last cell hold
            // every id of the row. Most cells hold the default style; skipping id 0 saves the
            // hashing.
            let cells = row.cells();
            let used = usize::from(row.header().occ).min(cells.len());
            for cell in cells
                .get(..used)
                .unwrap_or_default()
                .iter()
                .chain(cells.last())
            {
                let id = cell.style_id().0;
                if id != 0 {
                    out.insert(id);
                }
            }
        }
        for cursor in [&self.cursor, &self.saved_cursor] {
            out.insert(cursor.template().style_id().0);
            out.insert(cursor.erase().style_id().0);
        }
    }

    /// Adds to `out` the extras ids this screen holds: every cell of the rows flagged
    /// `HAS_EXTRAS` (history included; the hint has no false negatives) and both cursors' pens.
    pub(crate) fn collect_extras_ids(&self, out: &mut FxHashSet<u16>) {
        for row in self
            .slots
            .iter()
            .flatten()
            .filter(|row| row.header().flags.contains(RowFlags::HAS_EXTRAS))
        {
            // Cells from `occ` on equal the last one, so the prefix and the last cell hold
            // every id of the row. Most cells hold none; skipping id 0 saves the hashing.
            let cells = row.cells();
            let used = usize::from(row.header().occ).min(cells.len());
            for cell in cells
                .get(..used)
                .unwrap_or_default()
                .iter()
                .chain(cells.last())
            {
                let id = cell.extras_id().0;
                if id != 0 {
                    out.insert(id);
                }
            }
        }
        out.insert(self.cursor.template().extras_id().0);
        out.insert(self.saved_cursor.template().extras_id().0);
    }

    /// Rewrites the cluster ids of every live row flagged `HAS_GRAPHEME` through `remap`.
    pub(crate) fn remap_graphemes(&mut self, remap: &GraphemeRemap) {
        let mut id = self.oldest;
        while id <= self.newest {
            if self.row(id).flags().contains(RowFlags::HAS_GRAPHEME)
                && let Some(mut row) = self.row_mut(id)
            {
                row.remap_graphemes(remap);
            }
            id = id + 1;
        }
        self.debug_check();
    }

    /// The size of the ring in slots.
    pub fn ring_len(&self) -> usize {
        self.slots.len()
    }

    /// The mask that maps an id to its slot.
    pub fn ring_mask(&self) -> u64 {
        self.mask
    }

    /// Heap bytes held: the ring, every row (its box and its cells), the spare and the tab
    /// stops.
    pub fn heap_bytes(&self) -> usize {
        let rows: usize = self
            .slots
            .iter()
            .flatten()
            .chain(self.spare.as_ref())
            .map(|row| size_of::<Row>() + row.bytes())
            .sum();
        self.slots.len() * size_of::<Option<Box<Row>>>() + rows + self.tabs.stops.capacity()
    }

    /// Appends the text of a row: spacer cells skipped, clusters resolved, no trimming, no
    /// newline. A row never written gives `cols` spaces.
    pub fn row_text(&self, id: RowId, graphemes: &GraphemeArena, out: &mut String) {
        for cell in self.row(id).cells() {
            if cell.width().is_spacer() {
                continue;
            }
            match cell.content() {
                CellContent::Scalar(c) => out.push(c),
                CellContent::Grapheme(cluster) => out.push_str(graphemes.resolve(cluster)),
            }
        }
    }

    /// Tier 1 check after every mutating method of a debug build.
    fn debug_check(&self) {
        debug_assert_eq!(self.check_bounds(), Ok::<(), IntegrityViolation>(()));
        // `Anchors::trim` and `shift_region` leave screen-owned anchors alone on this promise.
        debug_assert!(
            self.owned_anchors()
                .iter()
                .all(|(_, pos)| pos.row >= self.oldest),
            "a screen-owned anchor sits below the oldest row"
        );
    }
}

#[cfg(test)]
mod screen_tests;
