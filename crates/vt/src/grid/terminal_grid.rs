//! The two screens of a terminal and the anchors they share.
//!
//! Region and tab stops are one setting in VT semantics but each screen owns a copy, so they
//! are copied across on every switch. The dispatcher composes the screen-switching modes from
//! `enter_alt`, `leave_alt`, `wipe` and `Screen::save_cursor`: `?47` is enter and leave;
//! `?1047` wipes before it leaves; `?1049` saves the primary cursor, enters, then wipes (so the
//! wipe uses the entering pen's background).

use crate::grid::anchor::Anchors;
use crate::grid::integrity::IntegrityViolation;
use crate::grid::row::{Row, SeqNo};
use crate::grid::screen::{DisplayClear, PrintMode, Screen, ScreenKind, ScrollReport};
use crate::grid::{RowId, ScrollRegion, Size};
use crate::intern::{GraphemeArena, Interner};

/// Where the content lands when the window changes size.
///
/// The two policies differ in how a resize treats the rows above and below the cursor; a viewport
/// that follows output stays at the bottom under both, and a scrolled-back viewport keeps its
/// top row.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum ResizePolicy {
    /// The bottom of the content stays at the bottom of the window: growing pulls history in at
    /// the top and the cursor moves down with its row. For programs that redraw after the
    /// resize signal (Unix PTYs, SSH).
    #[default]
    BottomAnchor,
    /// The top row of the screen stays put, as the Windows console host does behind ConPTY:
    /// the cursor lands on the row the host re-wraps it to, and new rows appear blank at the
    /// bottom. For local Windows shells.
    KeepViewportTop,
}

/// What a resize did.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct ResizeOutcome {
    /// The column count changed, so the primary screen was re-laid out. True even when every
    /// line kept its row.
    pub reflowed: bool,
    /// Primary rows that left the oldest end of the history during the call; anchors on them
    /// died. Rows released at the bottom are not counted.
    pub rows_trimmed: u32,
}

/// A primary and an alternate screen, their anchors and the batch stamp.
#[derive(Debug)]
pub struct TerminalGrid {
    primary: Screen,
    alt: Screen,
    alt_active: bool,
    anchors: Anchors,
    seq: SeqNo,
    /// Detached rows during a column reflow; empty between calls, capacity kept.
    reflow_rows: Vec<Option<Row>>,
}

impl TerminalGrid {
    /// Creates a grid of `size` (clamped); the primary screen keeps `scrollback_limit` rows of
    /// history, the alternate none. The stamp starts at `SeqNo(1)`.
    pub fn new(size: Size, scrollback_limit: u32) -> TerminalGrid {
        let mut anchors = Anchors::new();
        let primary = Screen::new(ScreenKind::Primary, size, scrollback_limit, &mut anchors);
        let alt = Screen::new(ScreenKind::Alternate, size, 0, &mut anchors);
        TerminalGrid {
            primary,
            alt,
            alt_active: false,
            anchors,
            seq: SeqNo(1),
            reflow_rows: Vec::new(),
        }
    }

    /// The primary screen, active or not.
    pub fn primary(&self) -> &Screen {
        &self.primary
    }

    /// The alternate screen, active or not.
    pub fn alt(&self) -> &Screen {
        &self.alt
    }

    /// Whether the alternate screen is the one shown.
    pub fn alt_active(&self) -> bool {
        self.alt_active
    }

    /// The active screen, for operations that need no anchors.
    pub fn screen_mut(&mut self) -> &mut Screen {
        self.active().0
    }

    /// The active screen.
    pub(crate) fn screen(&self) -> &Screen {
        if self.alt_active {
            &self.alt
        } else {
            &self.primary
        }
    }

    /// The screen whose lane holds `id`.
    pub(crate) fn screen_of(&self, id: RowId) -> &Screen {
        if id >= RowId::ALT_ORIGIN {
            &self.alt
        } else {
            &self.primary
        }
    }

    /// The anchors of both screens, for readers.
    pub(crate) fn anchors(&self) -> &Anchors {
        &self.anchors
    }

    /// The anchors of both screens, for owners of content anchors such as selections.
    pub(crate) fn anchors_mut(&mut self) -> &mut Anchors {
        &mut self.anchors
    }

    /// The active screen, borrowed apart from the anchors.
    fn active(&mut self) -> (&mut Screen, &mut Anchors) {
        let screen = if self.alt_active {
            &mut self.alt
        } else {
            &mut self.primary
        };
        (screen, &mut self.anchors)
    }

    /// Opens a batch of input: every row changed until the next call takes the returned stamp.
    pub fn begin_batch(&mut self) -> SeqNo {
        self.seq = self.seq.next();
        self.primary.set_seq(self.seq);
        self.alt.set_seq(self.seq);
        self.seq
    }

    /// The stamp of the current batch: every row changed since the last [`begin_batch`](Self::begin_batch)
    /// carries it, and a consumer that has read up to it needs only rows stamped higher.
    pub(crate) fn seq(&self) -> SeqNo {
        self.seq
    }

    /// Writes both screens' cursor and viewport anchors back from their fields. Operations
    /// that take no anchors leave them stale until this runs.
    pub fn sync_anchors(&mut self) {
        self.primary.sync_anchors(&mut self.anchors);
        self.alt.sync_anchors(&mut self.anchors);
    }

    /// Wraps the active screen's cursor to the next row; see [`Screen::wrapline`]. Returns
    /// the report of the scroll it ran, `Some` exactly when one did.
    pub fn wrapline(&mut self) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.wrapline(anchors)
    }

    /// `RIS`: resets both screens and shows the primary.
    pub fn reset(&mut self) {
        self.primary.reset(&mut self.anchors);
        self.alt.reset(&mut self.anchors);
        self.alt_active = false;
        self.sync_anchors();
        self.debug_check();
    }

    /// Resizes both screens to `size` (clamped to 1..=1024 rows and 1..=2048 columns) and
    /// moves the cursors, the viewport and the anchors with their content.
    ///
    /// A column change re-lays the primary screen out over its logical lines (rows joined by
    /// `WRAPPED`, wide glyphs never split, trailing blanks trimmed) and clips or pads the
    /// alternate screen. Every primary row then has a new id above all old ones, so follow
    /// content with anchors, not row ids. A row-only change keeps ids. Anchors die when their
    /// row falls off the oldest end of the history or is released below the cursor. The scroll
    /// region is reset to the whole screen. A resize to the current size does nothing.
    ///
    /// `policy` picks where the content lands; see [`ResizePolicy`].
    pub fn resize(&mut self, size: Size, policy: ResizePolicy) -> ResizeOutcome {
        let size = size.clamped();
        if size == self.primary.size() && size == self.alt.size() {
            return ResizeOutcome::default();
        }
        self.begin_batch();
        // The host measures where the cursor goes before anything moves.
        let host_row =
            (policy == ResizePolicy::KeepViewportTop).then(|| self.primary.host_cursor_row(size));
        let follows_output = self.primary.scroll_offset() == 0;
        let mut view_top = self.primary.visible_top();
        let mut outcome = ResizeOutcome::default();
        if size.cols != self.primary.cols() {
            let reflow =
                self.primary
                    .reflow_columns(size.cols, &mut self.anchors, &mut self.reflow_rows);
            outcome.reflowed = true;
            outcome.rows_trimmed = reflow.trimmed;
            view_top = reflow.view_top;
        }
        if size.cols != self.alt.cols() {
            self.alt.clip_columns(size.cols, &mut self.anchors);
        }
        let primary_trimmed = self.primary.resize_rows(size.rows, &mut self.anchors);
        // Whatever the alternate screen pushes is trimmed at once and not reported.
        self.alt.resize_rows(size.rows, &mut self.anchors);
        let mut shifted = 0;
        if let Some(host_row) = host_row {
            shifted = self.primary.shift_cursor_to(host_row, &mut self.anchors);
        }
        outcome.rows_trimmed = outcome
            .rows_trimmed
            .saturating_add(primary_trimmed)
            .saturating_add(shifted);
        self.primary.restate_viewport(follows_output, view_top);
        self.sync_anchors();
        self.debug_check();
        outcome
    }

    /// Heap bytes held by both screens, the anchors and the reflow scratch list.
    pub fn heap_bytes(&self) -> usize {
        self.primary.heap_bytes()
            + self.alt.heap_bytes()
            + self.anchors.heap_bytes()
            + self.reflow_rows.capacity() * size_of::<Option<Row>>()
    }

    pub(crate) fn linefeed(&mut self) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.linefeed(anchors)
    }

    pub(crate) fn reverse_index(&mut self) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.reverse_index(anchors)
    }

    pub(crate) fn scroll_up(&mut self, region: ScrollRegion, n: u16) -> ScrollReport {
        let (screen, anchors) = self.active();
        screen.scroll_up(region, n, anchors)
    }

    pub(crate) fn scroll_down(&mut self, region: ScrollRegion, n: u16) -> ScrollReport {
        let (screen, anchors) = self.active();
        screen.scroll_down(region, n, anchors)
    }

    pub(crate) fn insert_lines(&mut self, n: u16) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.insert_lines(n, anchors)
    }

    pub(crate) fn delete_lines(&mut self, n: u16) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.delete_lines(n, anchors)
    }

    pub(crate) fn print(
        &mut self,
        c: char,
        width: u8,
        mode: PrintMode,
        interner: &mut Interner,
    ) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.print(c, width, mode, interner, anchors)
    }

    pub(crate) fn print_narrow_run(
        &mut self,
        text: &str,
        mode: PrintMode,
        narrow: impl Fn(char) -> Option<char>,
    ) -> usize {
        self.active().0.print_narrow_run(text, mode, narrow)
    }

    pub(crate) fn put_tab(&mut self, count: u16, mode: PrintMode) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.put_tab(count, mode, anchors)
    }

    pub(crate) fn erase_display(
        &mut self,
        mode: DisplayClear,
        interner: &Interner,
    ) -> Option<ScrollReport> {
        let (screen, anchors) = self.active();
        screen.erase_display(mode, anchors, interner)
    }

    /// Shows the alternate screen. It adopts the primary's region and tab stops, and the
    /// primary's cursor at the same screen index. Does nothing if it is already shown.
    pub(crate) fn enter_alt(&mut self) {
        if self.alt_active {
            return;
        }
        self.alt.adopt_region_and_tabs(&self.primary);
        self.alt.adopt_cursor(&self.primary);
        self.alt_active = true;
        self.sync_anchors();
        self.debug_check();
    }

    /// Shows the primary screen again; it adopts the alternate's region and tab stops. Its
    /// cursor was frozen while hidden. Does nothing if it is already shown.
    pub(crate) fn leave_alt(&mut self) {
        if !self.alt_active {
            return;
        }
        self.primary.adopt_region_and_tabs(&self.alt);
        self.alt_active = false;
        self.sync_anchors();
        self.debug_check();
    }

    /// Replaces the content of the active screen: kills the anchors on its rows, follows
    /// output and blanks every row with the erase cell.
    pub(crate) fn wipe(&mut self) {
        let (screen, anchors) = self.active();
        screen.wipe(anchors);
        self.debug_check();
    }

    /// Changes the primary screen's scrollback limit; see [`Screen::set_scrollback_limit`]. The
    /// alternate screen has no history and is untouched.
    pub(crate) fn set_scrollback_limit(&mut self, limit: u32) {
        self.primary.set_scrollback_limit(limit, &mut self.anchors);
        self.debug_check();
    }

    /// Rebuilds the grapheme arena from the clusters the live rows of both screens still hold
    /// and rewrites those rows' ids. Call it between sequences, when
    /// [`Interner::needs_grapheme_sweep`] says so. Rewritten rows carry the current batch stamp.
    pub(crate) fn sweep_graphemes(&mut self, arena: &mut GraphemeArena) {
        let remap = arena.sweep(
            [&self.primary, &self.alt]
                .into_iter()
                .flat_map(Screen::grapheme_ids),
        );
        self.primary.remap_graphemes(&remap);
        self.alt.remap_graphemes(&remap);
        self.debug_check();
    }

    /// Tier 3 in a debug build, against the interner the cells point into; a no-op in release.
    /// Assumes the anchors are synced.
    pub(crate) fn debug_check_with(&self, interner: &Interner) {
        debug_assert_eq!(
            self.check_integrity(Some(interner)),
            Ok::<(), IntegrityViolation>(())
        );
    }

    /// Tier 3: both screens' bounds and rows, then the anchors. Assumes the anchors are synced.
    pub(crate) fn check_integrity(
        &self,
        interner: Option<&Interner>,
    ) -> Result<(), IntegrityViolation> {
        self.primary.check_rows(interner)?;
        self.alt.check_rows(interner)?;
        for (id, anchor) in self.anchors.iter() {
            if !anchor.alive || anchor.kind.is_screen_owned() {
                continue;
            }
            if !self
                .screen_of(anchor.pos.row)
                .row_range()
                .contains(&anchor.pos.row)
            {
                return Err(IntegrityViolation::AnchorOutsideLane { id });
            }
        }
        for screen in [&self.primary, &self.alt] {
            self.check_screen_anchors(screen)?;
        }
        Ok(())
    }

    /// A lane holds exactly three live screen-owned anchors, equal to its screen's fields.
    fn check_screen_anchors(&self, screen: &Screen) -> Result<(), IntegrityViolation> {
        let lane = match screen.kind() {
            ScreenKind::Primary => RowId::PRIMARY_ORIGIN..RowId::ALT_ORIGIN,
            ScreenKind::Alternate => RowId::ALT_ORIGIN..RowId(u64::MAX),
        };
        let live = self
            .anchors
            .iter()
            .filter(|(_, anchor)| {
                anchor.alive && anchor.kind.is_screen_owned() && lane.contains(&anchor.pos.row)
            })
            .count();
        let synced = screen
            .owned_anchors()
            .iter()
            .all(|&(id, pos)| self.anchors.get(id) == Some(pos));
        if live == 3 && synced {
            Ok(())
        } else {
            Err(IntegrityViolation::ScreenOwnedAnchors {
                kind: screen.kind(),
                live,
            })
        }
    }

    fn debug_check(&self) {
        debug_assert_eq!(self.check_integrity(None), Ok::<(), IntegrityViolation>(()));
    }
}

#[cfg(test)]
#[path = "grid_props.rs"]
mod grid_props;
#[cfg(test)]
#[path = "resize_props.rs"]
mod resize_props;
#[cfg(test)]
#[path = "resize_tests.rs"]
mod resize_tests;
#[cfg(test)]
#[path = "terminal_grid_tests.rs"]
mod terminal_grid_tests;
