//! The two screens of a terminal and the anchors they share.
//!
//! Region and tab stops are one setting in VT semantics but each screen owns a copy, so they
//! are copied across on every switch. The dispatcher composes the screen-switching modes from
//! `enter_alt`, `leave_alt`, `wipe` and `Screen::save_cursor`: `?47` is enter and leave;
//! `?1047` wipes before it leaves; `?1049` saves the primary cursor, enters, then wipes (so the
//! wipe uses the entering pen's background).

use crate::grid::anchor::Anchors;
use crate::grid::integrity::IntegrityViolation;
use crate::grid::row::SeqNo;
use crate::grid::screen::{DisplayClear, PrintMode, Screen, ScreenKind, ScrollReport};
use crate::grid::{RowId, ScrollRegion, Size};
use crate::intern::Interner;

/// A primary and an alternate screen, their anchors and the batch stamp.
#[derive(Debug)]
pub struct TerminalGrid {
    primary: Screen,
    alt: Screen,
    alt_active: bool,
    anchors: Anchors,
    seq: SeqNo,
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

    /// Writes both screens' cursor and viewport anchors back from their fields. Operations
    /// that take no anchors leave them stale until this runs.
    pub fn sync_anchors(&mut self) {
        self.primary.sync_anchors(&mut self.anchors);
        self.alt.sync_anchors(&mut self.anchors);
    }

    /// Wraps the active screen's cursor to the next row; see [`Screen::wrapline`].
    pub fn wrapline(&mut self) {
        let (screen, anchors) = self.active();
        screen.wrapline(anchors);
    }

    /// `RIS`: resets both screens and shows the primary.
    pub fn reset(&mut self) {
        self.primary.reset(&mut self.anchors);
        self.alt.reset(&mut self.anchors);
        self.alt_active = false;
        self.sync_anchors();
        self.debug_check();
    }

    /// Heap bytes held by both screens and the anchors.
    pub fn heap_bytes(&self) -> usize {
        self.primary.heap_bytes() + self.alt.heap_bytes() + self.anchors.heap_bytes()
    }

    pub(crate) fn linefeed(&mut self) {
        let (screen, anchors) = self.active();
        screen.linefeed(anchors);
    }

    pub(crate) fn reverse_index(&mut self) {
        let (screen, anchors) = self.active();
        screen.reverse_index(anchors);
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

    pub(crate) fn print(&mut self, c: char, width: u8, mode: PrintMode, interner: &mut Interner) {
        let (screen, anchors) = self.active();
        screen.print(c, width, mode, interner, anchors);
    }

    pub(crate) fn put_tab(&mut self, count: u16, mode: PrintMode) {
        let (screen, anchors) = self.active();
        screen.put_tab(count, mode, anchors);
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
#[path = "terminal_grid_tests.rs"]
mod terminal_grid_tests;
