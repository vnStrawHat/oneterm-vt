//! Rows, headers, and the read and write views over them.
//!
//! A row is a fixed header plus one `Vec<Cell>` of exactly `cols` cells. A ring slot that was
//! never written holds no row at all and reads as blanks.
//!
//! Two invariants make writes cheap and checkable:
//!
//! - **`occ`**: every cell at `col >= occ` equals the row's last cell, so [`Row::reset`] only
//!   rewrites the prefix a writer touched.
//! - **Wide pairs**: a `Wide` cell is always followed by its `WideSpacer`, and a spacer is
//!   always preceded by its `Wide`. [`RowMut`] is the only write path and keeps this true.

use std::ops::Range;

use bitflags::bitflags;

use crate::cell::{Cell, CellContent, CellWidth};
use crate::grid::integrity::IntegrityViolation;
use crate::grid::{MAX_COLS, RowId};
use crate::intern::{ExtrasId, GraphemeRemap, Interner, StyleId};

/// Shared storage for reading a row that was never written.
static BLANK_CELLS: [Cell; MAX_COLS as usize] = [Cell::EMPTY; MAX_COLS as usize];

/// A change stamp. One stamp covers a whole batch of input, not each mutation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct SeqNo(pub u64);

impl SeqNo {
    /// The next stamp; saturates.
    pub(crate) fn next(self) -> SeqNo {
        SeqNo(self.0.saturating_add(1))
    }
}

bitflags! {
    /// Per-row state and content hints.
    ///
    /// Content hints may be false positives but never false negatives.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct RowFlags: u8 {
        /// The row continues on the next row (soft wrap).
        const WRAPPED = 1 << 0;
        /// The row changed since a consumer last cleared the flag.
        const DIRTY = 1 << 1;
        /// Some cell has a non-default style.
        const STYLED = 1 << 2;
        /// Some cell holds a grapheme cluster.
        const HAS_GRAPHEME = 1 << 3;
        /// Some cell has non-`NONE` extras.
        const HAS_EXTRAS = 1 << 4;
        /// Some cell is covered by an image.
        const HAS_GRAPHIC = 1 << 5;
        /// The content hints, without `WRAPPED` and `DIRTY`.
        const CONTENT_HINTS = Self::STYLED.bits()
            | Self::HAS_GRAPHEME.bits()
            | Self::HAS_EXTRAS.bits()
            | Self::HAS_GRAPHIC.bits();
    }
}

fn hints_of(cell: Cell) -> RowFlags {
    let mut flags = RowFlags::empty();
    if cell.style_id() != StyleId::DEFAULT {
        flags |= RowFlags::STYLED;
    }
    if matches!(cell.content(), CellContent::Grapheme(_)) {
        flags |= RowFlags::HAS_GRAPHEME;
    }
    if cell.extras_id() != ExtrasId::NONE {
        flags |= RowFlags::HAS_EXTRAS;
    }
    flags
}

/// The cell as a blank narrow cell, keeping style and the other fields.
pub(crate) fn blank_of(cell: Cell) -> Cell {
    cell.with_content(CellContent::Scalar(' '))
        .with_width(CellWidth::Narrow)
}

/// The right half of a wide cell.
fn spacer_of(wide: Cell) -> Cell {
    wide.with_content(CellContent::Scalar(' '))
        .with_width(CellWidth::WideSpacer)
}

/// The fixed part of a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowHeader {
    /// The row's position in the output stream.
    pub id: RowId,
    /// The stamp of the last batch that changed the row.
    pub seq: SeqNo,
    /// State and content hints.
    pub flags: RowFlags,
    /// Cells at or past this column all equal the last cell.
    pub occ: u16,
}

/// One row of cells.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    header: RowHeader,
    cells: Vec<Cell>,
}

impl Row {
    /// Creates a row of `cols` copies of `template`, marked dirty.
    pub fn new(id: RowId, cols: u16, seq: SeqNo, template: Cell) -> Row {
        Row {
            header: RowHeader {
                id,
                seq,
                flags: RowFlags::DIRTY | hints_of(template),
                occ: 0,
            },
            cells: vec![template; usize::from(cols)],
        }
    }

    /// Builds a row for a new id out of a spare row's allocation. Nothing of the spare survives.
    pub(crate) fn recycle(
        spare: Option<Row>,
        id: RowId,
        cols: u16,
        seq: SeqNo,
        template: Cell,
    ) -> Row {
        let mut cells = spare.map(|row| row.cells).unwrap_or_default();
        cells.clear();
        cells.resize(usize::from(cols), template);
        Row {
            header: RowHeader {
                id,
                seq,
                flags: RowFlags::DIRTY | hints_of(template),
                occ: 0,
            },
            cells,
        }
    }

    /// Builds a row around a ready cell buffer. The caller promises that `occ` follows the
    /// row invariant; the integrity walk checks the pairs, `occ` and the hints afterwards.
    pub(crate) fn from_cells(
        id: RowId,
        seq: SeqNo,
        cells: Vec<Cell>,
        flags: RowFlags,
        occ: u16,
    ) -> Row {
        debug_assert!(usize::from(occ) <= cells.len());
        Row {
            header: RowHeader {
                id,
                seq,
                flags,
                occ,
            },
            cells,
        }
    }

    /// Gives up the row, keeping only its cell buffer for reuse.
    pub(crate) fn into_cells(self) -> Vec<Cell> {
        self.cells
    }

    /// The row's id.
    pub fn id(&self) -> RowId {
        self.header.id
    }

    /// The row's cells.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub(crate) fn header(&self) -> &RowHeader {
        &self.header
    }

    /// Gives the row a new position; its content is considered changed.
    pub(crate) fn rehome(&mut self, id: RowId, seq: SeqNo) {
        self.header.id = id;
        self.header.seq = seq;
        self.header.flags |= RowFlags::DIRTY;
    }

    /// Truncates or pads the row with `Cell::EMPTY` to `cols` cells and repairs the new edge:
    /// a `Wide` left in the last column, or a `LeadingWideSpacer` that is no longer there,
    /// becomes a blank. `WRAPPED` is kept; the row is stamped and `DIRTY`. A truncation keeps
    /// the buffer's capacity.
    pub(crate) fn fit(&mut self, cols: u16, seq: SeqNo) {
        let old_cols = self.cells.len();
        let new_cols = usize::from(cols);
        let old_last_blank = self.cells.last().is_none_or(|&cell| cell == Cell::EMPTY);
        self.cells.resize(new_cols, Cell::EMPTY);
        // A blank last cell means everything from `occ` on was blank, so the tail stays uniform.
        let mut occ = if old_last_blank {
            usize::from(self.header.occ).min(new_cols)
        } else {
            old_cols.min(new_cols)
        };
        // The old last column is inland only after a grow.
        let old_edge = old_cols.checked_sub(1).filter(|&edge| edge + 1 < new_cols);
        let new_edge = new_cols.checked_sub(1);
        for (index, broken) in [
            (old_edge, CellWidth::LeadingWideSpacer),
            (new_edge, CellWidth::Wide),
        ] {
            let Some(index) = index else { continue };
            if let Some(cell) = self.cells.get_mut(index)
                && cell.width() == broken
            {
                *cell = blank_of(*cell);
                occ = occ.max(index + 1);
            }
        }
        self.header.occ = u16::try_from(occ).unwrap_or(u16::MAX);
        self.header.seq = seq;
        self.header.flags |= RowFlags::DIRTY;
    }

    /// Fills the row with `template` and clears all state but `DIRTY`.
    pub fn reset(&mut self, template: Cell, seq: SeqNo) {
        let used = usize::from(self.header.occ);
        // Whole-cell compare: under the `occ` rule the untouched tail equals the last cell.
        let tail_is_template = used < self.cells.len() && self.cells.last() == Some(&template);
        match self.cells.get_mut(..used) {
            Some(prefix) if tail_is_template => prefix.fill(template),
            _ => self.cells.fill(template),
        }
        self.header.occ = 0;
        self.header.seq = seq;
        self.header.flags = RowFlags::DIRTY | hints_of(template);
    }

    /// The heap bytes the row's cells reserve.
    pub fn bytes(&self) -> usize {
        self.cells.capacity() * size_of::<Cell>()
    }

    /// Checks the row-local invariants of tier 2.
    pub(crate) fn check(
        &self,
        cols: u16,
        interner: Option<&Interner>,
    ) -> Result<(), IntegrityViolation> {
        let id = self.header.id;
        if self.cells.len() != usize::from(cols) {
            return Err(IntegrityViolation::RowWidth { id });
        }
        let occ = usize::from(self.header.occ);
        let last = self.cells.last().copied();
        let mut seen = RowFlags::empty();
        for (index, &cell) in self.cells.iter().enumerate() {
            let col = u16::try_from(index).unwrap_or(u16::MAX);
            let width_at = |at: Option<usize>| {
                at.and_then(|at| self.cells.get(at))
                    .map(|neighbour| neighbour.width())
            };
            match cell.width() {
                CellWidth::Wide if width_at(Some(index + 1)) != Some(CellWidth::WideSpacer) => {
                    return Err(IntegrityViolation::BrokenWidePair { id, col });
                }
                CellWidth::WideSpacer
                    if width_at(index.checked_sub(1)) != Some(CellWidth::Wide) =>
                {
                    return Err(IntegrityViolation::BrokenWidePair { id, col });
                }
                CellWidth::LeadingWideSpacer if index + 1 != self.cells.len() => {
                    return Err(IntegrityViolation::LeadingSpacerInland { id, col });
                }
                _ => {}
            }
            seen |= hints_of(cell);
            if index >= occ && Some(cell) != last {
                return Err(IntegrityViolation::OccTailNotUniform { id });
            }
            if let Some(interner) = interner {
                check_cell_ids(cell, interner).map_err(|kind| kind.at(id, col))?;
            }
        }
        let missing = seen.difference(self.header.flags);
        if !missing.is_empty() {
            return Err(IntegrityViolation::MissingHint { id, flag: missing });
        }
        Ok(())
    }
}

/// Which table an id of a cell is missing from.
#[derive(Clone, Copy)]
pub(crate) enum UnknownId {
    Style,
    Extras,
    Grapheme,
}

impl UnknownId {
    pub(crate) fn at(self, id: RowId, col: u16) -> IntegrityViolation {
        match self {
            UnknownId::Style => IntegrityViolation::UnknownStyle { id, col },
            UnknownId::Extras => IntegrityViolation::UnknownExtras { id, col },
            UnknownId::Grapheme => IntegrityViolation::UnknownGrapheme { id, col },
        }
    }
}

/// Checks that every id a cell holds resolves in `interner`.
pub(crate) fn check_cell_ids(cell: Cell, interner: &Interner) -> Result<(), UnknownId> {
    if usize::from(cell.style_id().0) >= interner.styles.entries() {
        return Err(UnknownId::Style);
    }
    if usize::from(cell.extras_id().0) >= interner.extras.entries() {
        return Err(UnknownId::Extras);
    }
    if let CellContent::Grapheme(id) = cell.content()
        && interner.resolve_grapheme(id).is_empty()
    {
        return Err(UnknownId::Grapheme);
    }
    Ok(())
}

/// What a reader gets for a row id: the row, or blanks for a row never written.
#[derive(Clone, Copy, Debug)]
pub struct RowRef<'a> {
    id: RowId,
    header: Option<&'a RowHeader>,
    cells: &'a [Cell],
}

impl<'a> RowRef<'a> {
    pub(crate) fn allocated(row: &'a Row) -> RowRef<'a> {
        RowRef {
            id: row.header.id,
            header: Some(&row.header),
            cells: &row.cells,
        }
    }

    pub(crate) fn blank(id: RowId, cols: u16) -> RowRef<'a> {
        RowRef {
            id,
            header: None,
            cells: BLANK_CELLS.get(..usize::from(cols)).unwrap_or(&[]),
        }
    }

    /// The row's id.
    pub fn id(&self) -> RowId {
        self.id
    }

    /// The row's cells; `cols` blanks for a row never written.
    pub fn cells(&self) -> &'a [Cell] {
        self.cells
    }

    /// The cell at `col`; `Cell::EMPTY` past the row's width.
    pub fn cell(&self, col: u16) -> Cell {
        self.cells
            .get(usize::from(col))
            .copied()
            .unwrap_or(Cell::EMPTY)
    }

    /// The row's flags; empty for a row never written.
    pub fn flags(&self) -> RowFlags {
        self.header.map_or(RowFlags::empty(), |header| header.flags)
    }

    /// Whether the row continues on the next row.
    pub fn wrapped(&self) -> bool {
        self.flags().contains(RowFlags::WRAPPED)
    }

    /// Cells at or past this column all equal the last cell; 0 for a row never written.
    pub fn occ(&self) -> u16 {
        self.header.map_or(0, |header| header.occ)
    }

    /// Whether the slot holds a row (false: never written, or trimmed).
    pub fn is_allocated(&self) -> bool {
        self.header.is_some()
    }

    /// One past the last column that is text: every column of a wrapped row, else the column
    /// after the last non-blank cell (0 for a blank row).
    pub(crate) fn text_end(&self) -> u16 {
        if self.wrapped() {
            return u16::try_from(self.cells.len()).unwrap_or(u16::MAX);
        }
        // Cells at or past `occ` equal the last cell, so a blank last cell lets the scan start there.
        let mut end = match self.cells.last() {
            Some(&last) if last.is_blank() => usize::from(self.occ()).min(self.cells.len()),
            _ => self.cells.len(),
        };
        while end > 0 && self.cells.get(end - 1).is_none_or(|&cell| cell.is_blank()) {
            end -= 1;
        }
        u16::try_from(end).unwrap_or(u16::MAX)
    }

    /// The stamp of the last batch that changed the row; `SeqNo(0)` for a row never written.
    pub(crate) fn seq(&self) -> SeqNo {
        self.header.map_or(SeqNo(0), |header| header.seq)
    }
}

/// The one write view of a row. It keeps wide pairs well formed, so no sequence of calls
/// can break the pair invariant.
#[derive(Debug)]
pub struct RowMut<'a> {
    row: &'a mut Row,
}

impl<'a> RowMut<'a> {
    /// Opens `row` for writing, stamping it with `seq` and `DIRTY`.
    pub fn new(row: &'a mut Row, seq: SeqNo) -> RowMut<'a> {
        row.header.seq = seq;
        row.header.flags |= RowFlags::DIRTY;
        RowMut { row }
    }

    /// The row's id.
    pub fn id(&self) -> RowId {
        self.row.header.id
    }

    /// The row's cells.
    pub fn cells(&self) -> &[Cell] {
        &self.row.cells
    }

    fn width_at(&self, index: usize) -> Option<CellWidth> {
        self.row.cells.get(index).map(|cell| cell.width())
    }

    /// Stores a cell as given, keeping `occ` and the hints right.
    fn store(&mut self, index: usize, cell: Cell) {
        if let Some(slot) = self.row.cells.get_mut(index) {
            *slot = cell;
            // `index < cols <= MAX_COLS`.
            let end = (index + 1) as u16;
            self.row.header.occ = self.row.header.occ.max(end);
            self.row.header.flags |= hints_of(cell);
        }
    }

    fn blank_cell(&mut self, index: usize) {
        if let Some(cell) = self.row.cells.get(index).copied() {
            self.store(index, blank_of(cell));
        }
    }

    /// Blanks the partner of the wide-pair half at `index`.
    fn release_pair(&mut self, index: usize) {
        match self.width_at(index) {
            Some(CellWidth::Wide) if self.width_at(index + 1) == Some(CellWidth::WideSpacer) => {
                self.blank_cell(index + 1);
            }
            Some(CellWidth::WideSpacer) => {
                if let Some(before) = index.checked_sub(1)
                    && self.width_at(before) == Some(CellWidth::Wide)
                {
                    self.blank_cell(before);
                }
            }
            _ => {}
        }
    }

    fn clear_wrapped_if_last(&mut self, end: usize) {
        if end >= self.row.cells.len() {
            self.row.header.flags.remove(RowFlags::WRAPPED);
        }
    }

    /// Writes `cell` at `col`; a column past the row is ignored.
    ///
    /// A wide cell also writes its spacer; a wide cell in the last column, and any stray
    /// spacer, degrades to a blank. The pair being overwritten is released first.
    pub fn set(&mut self, col: u16, cell: Cell) {
        let index = usize::from(col);
        let len = self.row.cells.len();
        if index >= len {
            return;
        }
        self.release_pair(index);
        let end = match cell.width() {
            CellWidth::Wide if index + 1 < len => {
                self.release_pair(index + 1);
                self.store(index, cell);
                self.store(index + 1, spacer_of(cell));
                index + 2
            }
            CellWidth::Narrow => {
                self.store(index, cell);
                index + 1
            }
            CellWidth::LeadingWideSpacer if index + 1 == len => {
                self.store(index, cell);
                index + 1
            }
            _ => {
                self.store(index, blank_of(cell));
                index + 1
            }
        };
        self.clear_wrapped_if_last(end);
    }

    /// Marks whether the row continues on the next row.
    pub fn set_wrapped(&mut self, wrapped: bool) {
        self.row.header.flags.set(RowFlags::WRAPPED, wrapped);
    }

    /// Fills the row with `template`; see [`Row::reset`].
    pub fn reset(&mut self, template: Cell, seq: SeqNo) {
        self.row.reset(template, seq);
    }

    /// Fills `cols` with `template`, splitting no pair.
    pub(crate) fn fill(&mut self, cols: Range<u16>, template: Cell) {
        let len = self.row.cells.len();
        let start = usize::from(cols.start).min(len);
        let end = usize::from(cols.end).min(len);
        if start >= end {
            return;
        }
        self.release_pair(start);
        self.release_pair(end - 1);
        if let Some(range) = self.row.cells.get_mut(start..end) {
            range.fill(template);
        }
        self.changed_up_to(end, template);
    }

    /// Records that cells up to `end` changed to `template`.
    fn changed_up_to(&mut self, end: usize, template: Cell) {
        // `end <= cols <= MAX_COLS`.
        self.row.header.occ = self.row.header.occ.max(end as u16);
        self.row.header.flags |= hints_of(template);
        self.clear_wrapped_if_last(end);
    }

    /// Deletes `n` cells at `col`, shifting the rest left and filling the end with `template`.
    pub(crate) fn delete_cells(&mut self, col: u16, n: u16, template: Cell) {
        let len = self.row.cells.len();
        let at = usize::from(col);
        if at >= len || n == 0 {
            return;
        }
        let n = usize::from(n).min(len - at);
        self.release_pair(at);
        self.row.cells.copy_within(at + n..len, at);
        if let Some(tail) = self.row.cells.get_mut(len - n..) {
            tail.fill(template);
        }
        // The spacer of a deleted `Wide` is now an orphan at `at`.
        if self.width_at(at) == Some(CellWidth::WideSpacer) {
            self.blank_cell(at);
        }
        // A leading spacer moved off the last column.
        if len - n > at
            && let Some(moved_last) = (len - n).checked_sub(1)
            && self.width_at(moved_last) == Some(CellWidth::LeadingWideSpacer)
        {
            self.blank_cell(moved_last);
        }
        self.changed_up_to(len, template);
    }

    /// Opens `n` cells at `col` filled with `template`, shifting the rest right.
    pub(crate) fn insert_cells(&mut self, col: u16, n: u16, template: Cell) {
        let len = self.row.cells.len();
        let at = usize::from(col);
        if at >= len || n == 0 {
            return;
        }
        let n = usize::from(n).min(len - at);
        if self.width_at(at) == Some(CellWidth::WideSpacer) {
            self.release_pair(at);
            self.blank_cell(at);
        }
        self.row.cells.copy_within(at..len - n, at + n);
        if let Some(opened) = self.row.cells.get_mut(at..at + n) {
            opened.fill(template);
        }
        // The spacer of a `Wide` in the new last column fell off the end.
        if let Some(last) = len.checked_sub(1)
            && self.width_at(last) == Some(CellWidth::Wide)
        {
            self.blank_cell(last);
        }
        self.changed_up_to(len, template);
    }

    /// Opens `n` blank cells at `col` before a print overwrites them (IRM).
    pub(crate) fn shift_right(&mut self, col: u16, n: u16) {
        self.insert_cells(col, n, Cell::EMPTY);
    }

    /// Stores `cell` as given, keeping `WRAPPED`. For the two in-place fixes (a tab marker,
    /// a cross-row spacer release); callers keep the width of the cell they replace.
    pub(crate) fn repair(&mut self, col: u16, cell: Cell) {
        self.store(usize::from(col), cell);
    }

    /// Rewrites the cluster ids of this row through `remap`, keeping widths and every other
    /// field. Returns whether any cell changed.
    pub(crate) fn remap_graphemes(&mut self, remap: &GraphemeRemap) -> bool {
        let mut changed = false;
        for cell in &mut self.row.cells {
            if let CellContent::Grapheme(old) = cell.content() {
                let new = remap.get(old);
                if new != old {
                    *cell = cell.with_content(CellContent::Grapheme(new));
                    changed = true;
                }
            }
        }
        changed
    }

    /// Marks that an image covers some cell of the row.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "Sixel placement marks rows from phase 10")
    )]
    pub(crate) fn mark_graphic(&mut self) {
        self.row.header.flags |= RowFlags::HAS_GRAPHIC;
    }
}

#[cfg(test)]
#[path = "row_tests.rs"]
mod row_tests;
