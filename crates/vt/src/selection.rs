//! Selections that follow their text.
//!
//! A [`Selection`] is two anchors in the grid's anchor list plus the half of the cell each
//! pointer event hit. Every row-moving primitive already updates anchors, so a selection follows
//! its text through region scrolls, scroll into history and reflow with no code of its own; when
//! an end's content dies, the selection resolves to nothing.
//!
//! Content erased in place keeps its row ids, so anchors cannot see it: ask
//! [`Selection::invalidated_by`] before running such an erase.

use crate::cell::CellWidth;
use crate::grid::{AnchorId, AnchorKind, Pos, RowRef, Screen, TerminalGrid};
use crate::intern::GraphemeArena;

mod expand;
mod text;

#[cfg(test)]
mod selection_props;
#[cfg(test)]
mod selection_tests;

/// The default word delimiters for [`SelectionKind::Semantic`]: space, tab, quotes, brackets,
/// and a few separators.
pub(crate) const DEFAULT_SEMANTIC_ESCAPE_CHARS: &str = ",│`|:\"' ()[]{}<>\t";

/// How a selection turns its two ends into cells.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SelectionKind {
    /// The cells between the two pointer positions, in reading order.
    Simple,
    /// The rectangle with the two positions as corners.
    Block,
    /// Whole logical lines (rows joined by the wrap flag) from the first end to the last.
    Lines,
    /// From the start of the word at the first end to the end of the word at the last.
    Semantic,
}

/// Which half of a cell the pointer hit.
///
/// For [`SelectionKind::Simple`] and [`SelectionKind::Block`], `Left` includes the cell at a
/// start and excludes it at an end; `Right` the reverse. `Semantic` and `Lines` ignore it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Side {
    /// The left half.
    Left,
    /// The right half.
    Right,
}

/// The cells a selection covers right now, both ends inclusive.
///
/// `start <= end` in row-major order, both inside the live rows of one screen with
/// `col < cols`; for a block also `start.col <= end.col`. It is a value: valid for the grid
/// state it was computed from, so ask again after the grid changes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SelectionRange {
    /// The first cell.
    pub start: Pos,
    /// The last cell, inclusive.
    pub end: Pos,
    /// Whether the range is the rectangle with `start` and `end` as corners.
    pub is_block: bool,
}

impl SelectionRange {
    /// Whether the cell at `pos` is inside the range.
    pub fn contains(&self, pos: Pos) -> bool {
        if self.is_block {
            (self.start.row..=self.end.row).contains(&pos.row)
                && (self.start.col..=self.end.col).contains(&pos.col)
        } else {
            (self.start..=self.end).contains(&pos)
        }
    }

    /// Whether `col` of `row` should be painted as selected: [`contains`](Self::contains), or
    /// the other half of a double-width glyph that is. A block edge that cuts a glyph paints
    /// the whole glyph, matching the copied text.
    pub fn contains_cell(&self, row: RowRef<'_>, col: u16) -> bool {
        let at = |col| self.contains(Pos { row: row.id(), col });
        if at(col) {
            return true;
        }
        match row.cell(col).width() {
            CellWidth::Wide => at(col.saturating_add(1)),
            CellWidth::WideSpacer => col > 0 && at(col - 1),
            _ => false,
        }
    }
}

/// An in-place change of content, asked about before it happens.
///
/// Embedders construct a value to pass to [`Selection::invalidated_by`].
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Invalidation {
    /// ED 2, DECALN, DECCOLM: the whole screen is blanked.
    EraseScreen,
    /// ED 1: the screen from its top through the cursor row.
    EraseAbove,
    /// ED 0: the cursor row through the bottom of the screen.
    EraseBelow,
    /// EL 0, 1 or 2: the cursor row.
    EraseLine,
    /// ED 3: the scrollback.
    EraseHistory,
    /// RIS: a full reset.
    Reset,
    /// A switch between the primary and the alternate screen.
    SwapAlt,
}

/// A selection: two anchors and the side each pointer event hit.
///
/// The selection owns two entries in the grid's anchor list. The owner must call
/// [`release`](Self::release) when done; dropping a `Selection` leaks the entries (dead or
/// alive) until the grid is dropped. Several selections on one grid are allowed.
#[derive(PartialEq, Eq, Debug)]
pub struct Selection {
    kind: SelectionKind,
    start: AnchorId,
    end: AnchorId,
    start_side: Side,
    end_side: Side,
}

/// One end of a selection as the pointer left it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct End {
    pos: Pos,
    side: Side,
}

/// Returns `a` and `b` ordered by position; an end keeps its side.
fn ordered(a: End, b: End) -> (End, End) {
    if a.pos <= b.pos { (a, b) } else { (b, a) }
}

impl Selection {
    /// Starts a selection of `kind` at `pos`, both ends on it and both sides `side`.
    ///
    /// `pos` clamps into the live rows of its screen and to its last column. The new
    /// selection is empty for `Simple` and `Block`; `Semantic` and `Lines` select the word or
    /// line at once. Call [`release`](Self::release) when done.
    pub fn new(grid: &mut TerminalGrid, kind: SelectionKind, pos: Pos, side: Side) -> Selection {
        let pos = clamp(grid, pos);
        let anchors = grid.anchors_mut();
        Selection {
            kind,
            start: anchors.register(AnchorKind::SelectionStart, pos),
            end: anchors.register(AnchorKind::SelectionEnd, pos),
            start_side: side,
            end_side: side,
        }
    }

    /// Moves the end of the selection to `pos` (clamped like [`new`](Self::new)) with `side`.
    ///
    /// This also revives an end whose content died.
    pub fn update(&mut self, grid: &mut TerminalGrid, pos: Pos, side: Side) {
        let pos = clamp(grid, pos);
        grid.anchors_mut().set(self.end, pos);
        self.end_side = side;
    }

    /// Frees both anchors. Must be called exactly once, before the selection is dropped.
    pub fn release(self, grid: &mut TerminalGrid) {
        let anchors = grid.anchors_mut();
        anchors.release(self.start);
        anchors.release(self.end);
    }

    /// Selects everything on the active screen, history included: a `Simple` selection from the
    /// first cell of the oldest row to the last cell of the newest. Call
    /// [`release`](Self::release) when done.
    pub fn all(grid: &mut TerminalGrid) -> Selection {
        let screen = grid.screen();
        let first = Pos {
            row: screen.oldest(),
            col: 0,
        };
        let last = Pos {
            row: screen.newest(),
            col: screen.cols().saturating_sub(1),
        };
        let anchors = grid.anchors_mut();
        Selection {
            kind: SelectionKind::Simple,
            start: anchors.register(AnchorKind::SelectionStart, first),
            end: anchors.register(AnchorKind::SelectionEnd, last),
            start_side: Side::Left,
            end_side: Side::Right,
        }
    }

    /// Returns the cells selected now, or `None` when there are none to show: an end's content
    /// is gone (scrolled out of a region, trimmed, wiped or resized away), the ends sit on
    /// different screens, or a `Simple` or `Block` selection covers no whole cell.
    ///
    /// `escape_chars` are the word delimiters for [`SelectionKind::Semantic`]; a cell holding a
    /// grapheme cluster never is one. A word or line longer than 65,536 cells (rows) is cut, as
    /// is a bracket search. Pairs of double-width cells are never split in a non-block range.
    /// Does not allocate.
    pub fn to_range(&self, grid: &TerminalGrid, escape_chars: &str) -> Option<SelectionRange> {
        let anchors = grid.anchors();
        let (a, b) = (anchors.get(self.start)?, anchors.get(self.end)?);
        let screen = grid.screen_of(a.row);
        if screen.kind() != grid.screen_of(b.row).kind() {
            return None;
        }
        let last_col = screen.cols().saturating_sub(1);
        let end_of = |pos: Pos, side| End {
            pos: Pos {
                row: pos.row,
                col: pos.col.min(last_col),
            },
            side,
        };
        let (a, b) = (end_of(a, self.start_side), end_of(b, self.end_side));
        let (start, end) = match self.kind {
            SelectionKind::Simple => simple_span(screen, a, b)?,
            SelectionKind::Block => return block_range(a, b),
            SelectionKind::Semantic => {
                let (lo, hi) = (a.pos.min(b.pos), a.pos.max(b.pos));
                let matched = (lo == hi)
                    .then(|| expand::bracket_match(screen, lo, expand::EXPAND_BUDGET))
                    .flatten();
                match matched {
                    Some(other) => (lo.min(other), lo.max(other)),
                    None => (
                        expand::word_start(screen, lo, escape_chars, expand::EXPAND_BUDGET),
                        expand::word_end(screen, hi, escape_chars, expand::EXPAND_BUDGET),
                    ),
                }
            }
            SelectionKind::Lines => {
                let (lo, hi) = (a.pos.min(b.pos), a.pos.max(b.pos));
                (
                    expand::line_start(screen, lo, expand::EXPAND_BUDGET),
                    expand::line_end(screen, hi, expand::EXPAND_BUDGET),
                )
            }
        };
        let (start, end) = snap_wide_pairs(screen, start, end);
        Some(SelectionRange {
            start,
            end,
            is_block: false,
        })
    }

    /// Returns the selected text, or `None` exactly when [`to_range`](Self::to_range) does.
    ///
    /// Wrapped rows join without a newline and trailing blanks of a row are dropped; a tab
    /// and the blanks it skipped copy as one `'\t'`; double-width glyphs and clusters come out
    /// whole. `Lines` text ends with one `'\n'`; no other kind's does. A block copies each row's
    /// slice as its own line. `graphemes` resolves clusters.
    pub fn text(
        &self,
        grid: &TerminalGrid,
        graphemes: &GraphemeArena,
        escape_chars: &str,
    ) -> Option<String> {
        let range = self.to_range(grid, escape_chars)?;
        let screen = grid.screen_of(range.start.row);
        Some(text::selection_text(screen, graphemes, self.kind, range))
    }

    /// Whether either end's content is gone. A fresh empty selection is not dead, so callers
    /// prune on this and never on `to_range() == None`.
    pub(crate) fn is_dead(&self, grid: &TerminalGrid) -> bool {
        let anchors = grid.anchors();
        anchors.get(self.start).is_none() || anchors.get(self.end).is_none()
    }

    /// Whether erasing in place would leave this selection over blanked cells, so the embedder
    /// should clear it. Ask **before** running the erase, with the cursor where the erase
    /// starts; judged against the selection's own screen and that screen's cursor.
    ///
    /// Row-level: an EL 0 to the right of the selection on the same row still counts. A
    /// selection with a dead end, and every `EraseScreen`, `Reset` and `SwapAlt`, always do.
    /// Writing over selected cells, ECH, DCH and ICH do not: the selection copies what the cells
    /// hold now. Scrolls, region edits, trims and resizes need no call; they move or kill the
    /// anchors.
    pub fn invalidated_by(&self, grid: &TerminalGrid, inv: Invalidation) -> bool {
        let anchors = grid.anchors();
        let (Some(a), Some(b)) = (anchors.get(self.start), anchors.get(self.end)) else {
            return true;
        };
        let (top, bottom) = (a.row.min(b.row), a.row.max(b.row));
        let screen = grid.screen_of(top);
        let cursor = screen.cursor().pos.row;
        match inv {
            Invalidation::Reset | Invalidation::SwapAlt | Invalidation::EraseScreen => true,
            Invalidation::EraseLine => top <= cursor && cursor <= bottom,
            Invalidation::EraseBelow => bottom >= cursor,
            Invalidation::EraseAbove => top <= cursor && bottom >= screen.screen_top(),
            Invalidation::EraseHistory => top < screen.screen_top(),
        }
    }
}

/// Clamps `pos` into the live rows of its screen and to the last column, so every live
/// content anchor stays inside its lane.
fn clamp(grid: &TerminalGrid, pos: Pos) -> Pos {
    let screen = grid.screen_of(pos.row);
    Pos {
        row: pos.row.clamp(screen.oldest(), screen.newest()),
        col: pos.col.min(screen.cols().saturating_sub(1)),
    }
}

/// The cells of a `Simple` selection, or `None` when no whole cell lies between the ends.
///
/// The end steps back before the start steps on, so `(r, last, Right)..(r + 1, 0, Left)` keeps
/// cell `(r, last)`.
fn simple_span(screen: &Screen, a: End, b: End) -> Option<(Pos, Pos)> {
    let (a, b) = ordered(a, b);
    let touching_halves = a.pos.row == b.pos.row
        && a.side == Side::Right
        && b.side == Side::Left
        && a.pos.col.saturating_add(1) == b.pos.col;
    if a == b || touching_halves {
        return None;
    }
    let (mut start, mut end) = (a.pos, b.pos);
    if b.side == Side::Left && start != end {
        end = expand::prev(screen, end)?;
    }
    if a.side == Side::Right && start != end {
        start = expand::next(screen, start)?;
    }
    (start <= end).then_some((start, end))
}

/// The rectangle of a `Block` selection, or `None` when no whole column lies between the ends.
fn block_range(a: End, b: End) -> Option<SelectionRange> {
    let (mut a, mut b) = ordered(a, b);
    let (left, right) = (a.pos.col, b.pos.col);
    let touching_halves =
        (left.saturating_add(1) == right && a.side == Side::Right && b.side == Side::Left)
            || (right.saturating_add(1) == left && a.side == Side::Left && b.side == Side::Right);
    if (left == right && a.side == b.side) || touching_halves {
        return None;
    }
    if a.pos.col > b.pos.col {
        (a.pos.col, b.pos.col) = (b.pos.col, a.pos.col);
        (a.side, b.side) = (b.side, a.side);
    }
    if b.side == Side::Left && a.pos != b.pos && b.pos.col > 0 {
        b.pos.col -= 1;
    }
    if a.side == Side::Right && a.pos != b.pos {
        a.pos.col = a.pos.col.saturating_add(1);
    }
    (a.pos.col <= b.pos.col).then_some(SelectionRange {
        start: a.pos,
        end: b.pos,
        is_block: true,
    })
}

/// Widens a non-block span so it never holds half of a double-width pair, and reaches the
/// wrapped glyph a leading spacer stands for.
fn snap_wide_pairs(screen: &Screen, start: Pos, end: Pos) -> (Pos, Pos) {
    let mut start = start;
    if screen.row(start.row).cell(start.col).width() == CellWidth::WideSpacer {
        start.col = start.col.saturating_sub(1);
    }
    let mut end = end;
    let row = screen.row(end.row);
    if row.cell(end.col).width() == CellWidth::LeadingWideSpacer
        && row.wrapped()
        && end.row < screen.newest()
    {
        end = Pos {
            row: end.row + 1,
            col: 0,
        };
    }
    if screen.row(end.row).cell(end.col).width() == CellWidth::Wide {
        end.col = end
            .col
            .saturating_add(1)
            .min(screen.cols().saturating_sub(1));
    }
    (start, end)
}
