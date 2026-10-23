//! Column reflow of the primary screen, and the clip of the alternate screen.
//!
//! A **logical line** is what the program printed between two line ends; rows are one layout
//! of it, and a width change lays it out again. Rows that continue (`WRAPPED`) are joined, the
//! trailing blanks are trimmed, and the text is cut into rows of the new width. Cells are
//! copied by value, so styles, links and grapheme ids survive without any interner access.
//!
//! The live rows are detached into a scratch list first: the output starts above every old id,
//! and a shrink writes more rows than it reads, so writing into the ring while the source is
//! still in it could overwrite unread rows.

use crate::cell::{Cell, CellWidth};
use crate::grid::anchor::{AnchorId, Anchors};
use crate::grid::row::{Row, RowFlags, RowMut, RowRef, blank_of};
use crate::grid::screen::{Cursor, Screen};
use crate::grid::{Pos, RowId, Size};

/// What a column reflow tells the caller.
pub(crate) struct Reflowed {
    /// Rows that fell off the oldest end because the output exceeded the scrollback limit.
    pub trimmed: u32,
    /// The id the viewport's top row now has (the oldest id when its content was trimmed).
    pub view_top: RowId,
}

/// Reads a row of the old layout; a slot without a row reads as blanks.
type RowAt<'a> = &'a dyn Fn(usize) -> RowRef<'a>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PointOwner {
    Anchor(AnchorId),
    Cursor,
    SavedCursor,
    ViewportTop,
}

/// A position the reflow carries from the old layout to the new one.
struct Point {
    owner: PointOwner,
    key: Pos,
    /// Offset in its logical line, set while the line is processed; clamped to the line length.
    offset: usize,
    /// Where the point landed; `None` while unplaced or when its row did not survive.
    dest: Option<Pos>,
}

impl Point {
    fn new(owner: PointOwner, key: Pos) -> Point {
        Point {
            owner,
            key,
            offset: 0,
            dest: None,
        }
    }
}

/// One output row of a laid-out line: the line offsets `start..end` it holds, a
/// `LeadingWideSpacer` after them when `leading_spacer` is set, and whether the line goes on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct RowSpan {
    start: usize,
    end: usize,
    leading_spacer: bool,
    wrapped: bool,
}

/// A logical line of the old layout: the id of its first row and how many rows it spans.
#[derive(Clone, Copy)]
struct Line {
    first_id: RowId,
    count: usize,
}

/// State shared by every line of one reflow.
struct Walk {
    old_cols: u16,
    new_cols: u16,
    old_oldest: RowId,
    /// Live rows the ring may hold: scrollback plus the screen.
    cap: u64,
    /// Scratch entries before this index are finished lines whose buffers may be reused.
    pool: usize,
    trimmed: u32,
}

/// A row's share of its logical line: its cells minus a trailing `LeadingWideSpacer`, which
/// stands in for a glyph that wrapped and which the new layout re-inserts where it needs one.
fn contribution(row: &RowRef<'_>) -> usize {
    let cells = row.cells();
    let filler = cells
        .last()
        .is_some_and(|cell| cell.width() == CellWidth::LeadingWideSpacer);
    cells.len().saturating_sub(usize::from(filler))
}

/// The index of the last cell of `row` before `limit` that is not `Cell::EMPTY`. The scan stops
/// at `occ` when the last cell is blank, so its cost is the blanks inside `occ`, not `cols`.
fn last_content(row: &RowRef<'_>, limit: usize) -> Option<usize> {
    let cells = row.cells();
    let scan = if cells.last() == Some(&Cell::EMPTY) {
        usize::from(row.occ())
    } else {
        cells.len()
    };
    cells
        .get(..scan.min(limit))?
        .iter()
        .rposition(|&cell| cell != Cell::EMPTY)
}

/// Returns the length of the line without trailing blanks.
fn line_length(row_at: RowAt<'_>, count: usize) -> usize {
    let total: usize = (0..count).map(|index| contribution(&row_at(index))).sum();
    let mut end = total;
    for index in (0..count).rev() {
        let row = row_at(index);
        let share = contribution(&row);
        let start = end.saturating_sub(share);
        if let Some(last) = last_content(&row, share) {
            return start + last + 1;
        }
        end = start;
    }
    0
}

/// A forward-only reader of the cells of one logical line, by line offset.
struct Reader<'a> {
    row_at: RowAt<'a>,
    count: usize,
    index: usize,
    start: usize,
    end: usize,
    current: RowRef<'a>,
}

impl<'a> Reader<'a> {
    fn new(row_at: RowAt<'a>, count: usize) -> Reader<'a> {
        let current = row_at(0);
        Reader {
            row_at,
            count,
            index: 0,
            start: 0,
            end: contribution(&current),
            current,
        }
    }

    /// Moves to the row holding `offset`; false when the line ends before it. Offsets must not
    /// go backwards.
    fn seek(&mut self, offset: usize) -> bool {
        while offset >= self.end {
            if self.index + 1 >= self.count {
                return false;
            }
            self.index += 1;
            self.start = self.end;
            self.current = (self.row_at)(self.index);
            self.end = self.start + contribution(&self.current);
        }
        true
    }

    fn cell_at(&mut self, offset: usize) -> Option<Cell> {
        if !self.seek(offset) {
            return None;
        }
        self.current
            .cells()
            .get(offset.saturating_sub(self.start))
            .copied()
    }

    /// Appends the cells at offsets `from..to`, one slice per source row crossed, and adds the
    /// content hints of those rows to `hints`.
    fn copy(&mut self, from: usize, to: usize, out: &mut Vec<Cell>, hints: &mut RowFlags) {
        let mut at = from;
        while at < to && self.seek(at) {
            let stop = to.min(self.end);
            let range = at.saturating_sub(self.start)..stop.saturating_sub(self.start);
            if let Some(slice) = self.current.cells().get(range) {
                out.extend_from_slice(slice);
            }
            *hints |= self.current.flags() & RowFlags::CONTENT_HINTS;
            at = stop;
        }
    }
}

/// Cuts a line of `length` cells into rows of `cols`, calling `on_row` for each.
///
/// A wide glyph never splits: when its first half would take the last column, that column
/// becomes a `LeadingWideSpacer` and the glyph starts the next row. An empty line is one row.
fn lay_out(line: &mut Reader<'_>, length: usize, cols: u16, mut on_row: impl FnMut(RowSpan)) {
    let cols = usize::from(cols);
    let mut start = 0usize;
    loop {
        let full = start.saturating_add(cols);
        // The glyph's second half is at `full`, so it is inside the line.
        let leading_spacer = cols >= 2
            && full < length
            && line
                .cell_at(full - 1)
                .is_some_and(|cell| cell.width() == CellWidth::Wide);
        let end = if leading_spacer {
            full - 1
        } else {
            full.min(length)
        };
        on_row(RowSpan {
            start,
            end,
            leading_spacer,
            wrapped: end < length,
        });
        if end >= length {
            return;
        }
        start = end;
    }
}

fn source_row(rows: &[Option<Row>], index: usize, id: RowId, cols: u16) -> RowRef<'_> {
    match rows.get(index).and_then(Option::as_ref) {
        Some(row) => RowRef::allocated(row),
        None => RowRef::blank(id, cols),
    }
}

/// Takes the buffer of a finished source row, scanning forward from `pool`.
fn take_pooled(done: &mut [Option<Row>], pool: &mut usize) -> Option<Row> {
    while let Some(slot) = done.get_mut(*pool) {
        if let Some(row) = slot.take() {
            return Some(row);
        }
        *pool += 1;
    }
    None
}

/// Gives every point of a line its offset in the line, and extends `length` to the cursor.
fn place_points(
    row_at: RowAt<'_>,
    count: usize,
    first_id: RowId,
    points: &mut [Point],
    length: &mut usize,
) {
    let (mut row, mut row_start) = (0usize, 0usize);
    for point in points.iter_mut() {
        let target = usize::try_from(point.key.row.distance(first_id))
            .unwrap_or(usize::MAX)
            .min(count.saturating_sub(1));
        while row < target {
            row_start += contribution(&row_at(row));
            row += 1;
        }
        point.offset = row_start + usize::from(point.key.col);
        if point.owner == PointOwner::Cursor {
            *length = (*length).max(point.offset);
        }
    }
    // A point on trimmed blanks lands at the end of its line.
    for point in points {
        point.offset = point.offset.min(*length);
    }
}

/// Puts a cursor on the cell a point landed on; a cell past the last column is a pending wrap.
fn land(cursor: &mut Cursor, dest: Option<Pos>, cols: u16, newest: RowId) {
    let Some(dest) = dest else {
        cursor.pos = Pos {
            row: newest,
            col: 0,
        };
        cursor.pending_wrap = false;
        return;
    };
    cursor.pending_wrap = dest.col >= cols;
    cursor.pos = Pos {
        row: dest.row,
        col: dest.col.min(cols.saturating_sub(1)),
    };
}

impl Screen {
    /// Re-lays the primary screen out at `cols` columns.
    ///
    /// Every live row gets a new id above all old ones; anchors, both cursors and the
    /// viewport top follow their cells. Rows the scrollback limit cannot hold fall off the
    /// oldest end, and anchors on them die. The row count is the caller's to change.
    pub(crate) fn reflow_columns(
        &mut self,
        cols: u16,
        anchors: &mut Anchors,
        scratch: &mut Vec<Option<Row>>,
    ) -> Reflowed {
        let mut points = self.collect_points(anchors);
        let mut walk = Walk {
            old_cols: self.cols,
            new_cols: cols,
            old_oldest: self.oldest,
            cap: u64::from(self.scrollback_limit) + u64::from(self.rows),
            pool: 0,
            trimmed: 0,
        };
        self.detach_rows(scratch);
        // The output is empty: `newest` sits one below the first output id.
        self.oldest = self.newest_issued + 1;
        self.newest = self.newest_issued;
        let mut first = 0usize;
        let mut placed = 0usize;
        while first < scratch.len() {
            let (done, rest) = scratch.split_at_mut(first);
            let count = line_rows(rest);
            let last_id = walk.old_oldest + (first + count - 1) as u64;
            let claimed = points.get(placed..).map_or(0, |tail| {
                tail.partition_point(|point| point.key.row <= last_id)
            });
            let line_points = points.get_mut(placed..placed + claimed).unwrap_or_default();
            let line = Line {
                first_id: walk.old_oldest + first as u64,
                count,
            };
            self.reflow_line(&mut walk, done, rest, line, line_points);
            first += count;
            placed += claimed;
        }
        self.finish_reflow(&walk, scratch, &points, anchors)
    }

    /// Points to carry: every live content anchor of the primary lane, the cursors and the
    /// top of the viewport, sorted by position.
    fn collect_points(&self, anchors: &Anchors) -> Vec<Point> {
        let lane = self.kind.origin()..RowId::ALT_ORIGIN;
        let mut points = Vec::with_capacity(anchors.live_in(lane.clone()) + 3);
        for (id, anchor) in anchors.iter() {
            if anchor.alive && !anchor.kind.is_screen_owned() && lane.contains(&anchor.pos.row) {
                points.push(Point::new(PointOwner::Anchor(id), anchor.pos));
            }
        }
        // A pending wrap means "past the last column".
        let key = |cursor: &Cursor| Pos {
            row: cursor.pos.row,
            col: if cursor.pending_wrap {
                self.cols
            } else {
                cursor.pos.col
            },
        };
        points.push(Point::new(PointOwner::Cursor, key(&self.cursor)));
        points.push(Point::new(PointOwner::SavedCursor, key(&self.saved_cursor)));
        // The screen-owned anchor entries may be stale, so the fields are the truth.
        let view = Pos {
            row: self.visible_top(),
            col: 0,
        };
        points.push(Point::new(PointOwner::ViewportTop, view));
        points.sort_unstable_by_key(|point| point.key);
        points
    }

    /// Moves every live row out of the ring into `scratch`, in id order.
    fn detach_rows(&mut self, scratch: &mut Vec<Option<Row>>) {
        scratch.clear();
        let live = self.newest.distance(self.oldest).saturating_add(1);
        scratch.reserve(usize::try_from(live).unwrap_or(0));
        let mut id = self.oldest;
        while id <= self.newest {
            let index = self.slot_index(id);
            scratch.push(self.slots.get_mut(index).and_then(Option::take));
            id = id + 1;
        }
    }

    /// Writes one logical line of the old layout into the new one.
    fn reflow_line(
        &mut self,
        walk: &mut Walk,
        done: &mut [Option<Row>],
        rest: &mut [Option<Row>],
        line: Line,
        points: &mut [Point],
    ) {
        let Line { first_id, count } = line;
        let (old_cols, new_cols) = (walk.old_cols, walk.new_cols);
        let (length, moves_whole) = {
            let rows: &[Option<Row>] = rest;
            let row_at = |index: usize| source_row(rows, index, first_id + index as u64, old_cols);
            let mut length = line_length(&row_at, count);
            place_points(&row_at, count, first_id, points, &mut length);
            // A single row that fits keeps its buffer. A trailing filler would be left behind
            // as a stray blank, so such a row goes through the layout.
            let whole = count == 1
                && (1..=usize::from(new_cols)).contains(&length)
                && rows.first().and_then(Option::as_ref).is_some_and(|row| {
                    row.cells()
                        .last()
                        .is_none_or(|cell| cell.width() != CellWidth::LeadingWideSpacer)
                });
            (length, whole)
        };
        if moves_whole {
            self.move_row(walk, rest.first_mut().and_then(Option::take), points);
        } else {
            self.copy_line(walk, done, rest, line, length, points);
        }
    }

    /// The fast path: the row is re-homed with its buffer, trailing blanks cut or padded.
    fn move_row(&mut self, walk: &mut Walk, row: Option<Row>, points: &mut [Point]) {
        let Some(mut row) = row else { return };
        let id = self.newest + 1;
        row.fit(walk.new_cols, self.seq);
        RowMut::new(&mut row, self.seq).set_wrapped(false);
        row.rehome(id, self.seq);
        for point in points {
            let col = u16::try_from(point.offset).unwrap_or(u16::MAX);
            point.dest = Some(Pos { row: id, col });
        }
        self.push_reflowed(Some(row), walk);
    }

    /// The general path: lays the line out and copies each output row from the old rows.
    fn copy_line(
        &mut self,
        walk: &mut Walk,
        done: &mut [Option<Row>],
        rest: &[Option<Row>],
        line: Line,
        length: usize,
        points: &mut [Point],
    ) {
        let Line { first_id, count } = line;
        let (old_cols, new_cols) = (walk.old_cols, walk.new_cols);
        let row_at = |index: usize| source_row(rest, index, first_id + index as u64, old_cols);
        let mut measure = Reader::new(&row_at, count);
        let mut copy = Reader::new(&row_at, count);
        let mut next_point = 0usize;
        lay_out(&mut measure, length, new_cols, |span| {
            let id = self.newest + 1;
            let row = self.build_row(walk, done, &mut copy, span, id);
            while let Some(point) = points.get_mut(next_point)
                && (!span.wrapped || point.offset < span.end)
            {
                let col =
                    u16::try_from(point.offset.saturating_sub(span.start)).unwrap_or(u16::MAX);
                point.dest = Some(Pos { row: id, col });
                next_point += 1;
            }
            self.push_reflowed(row, walk);
        });
    }

    /// Builds the output row for `span`; `None` for a row with nothing on it.
    fn build_row(
        &mut self,
        walk: &mut Walk,
        done: &mut [Option<Row>],
        copy: &mut Reader<'_>,
        span: RowSpan,
        id: RowId,
    ) -> Option<Row> {
        if span.start == span.end && !span.leading_spacer && !span.wrapped {
            return None;
        }
        let cols = usize::from(walk.new_cols);
        let mut cells = self
            .spare
            .take()
            .or_else(|| take_pooled(done, &mut walk.pool))
            .map(Row::into_cells)
            .unwrap_or_default();
        cells.clear();
        cells.reserve_exact(cols);
        let mut hints = RowFlags::empty();
        copy.copy(span.start, span.end, &mut cells, &mut hints);
        if cols == 1 {
            self.degrade_wide(&mut cells);
        }
        if span.leading_spacer {
            cells.push(Cell::EMPTY.with_width(CellWidth::LeadingWideSpacer));
        }
        let occ = u16::try_from(cells.len().min(cols)).unwrap_or(u16::MAX);
        cells.resize(cols, Cell::EMPTY);
        let mut flags = RowFlags::DIRTY | hints;
        flags.set(RowFlags::WRAPPED, span.wrapped);
        Some(Row::from_cells(id, self.seq, cells, flags, occ))
    }

    /// One column cannot hold a wide glyph: each half becomes a blank, and the glyph is counted.
    fn degrade_wide(&mut self, cells: &mut [Cell]) {
        for cell in cells {
            match cell.width() {
                CellWidth::Wide => {
                    self.dropped_wide = self.dropped_wide.saturating_add(1);
                    *cell = blank_of(*cell);
                }
                CellWidth::WideSpacer => *cell = blank_of(*cell),
                CellWidth::Narrow | CellWidth::LeadingWideSpacer => {}
            }
        }
    }

    /// Stores the next output row, first dropping the oldest one when the ring is full. The
    /// scrollback cap is applied in `finish_reflow`, after the blank tail is discarded: padding
    /// rows below the cursor must not push real content out.
    fn push_reflowed(&mut self, row: Option<Row>, walk: &mut Walk) {
        if (self.newest + 1).distance(self.oldest) >= self.ring_len() as u64 {
            let index = self.slot_index(self.oldest);
            if let Some(old) = self.slots.get_mut(index).and_then(Option::take) {
                self.spare = Some(old);
            }
            self.oldest = self.oldest + 1;
            walk.trimmed = walk.trimmed.saturating_add(1);
        }
        self.newest = self.newest + 1;
        let index = self.slot_index(self.newest);
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = row;
        }
    }

    /// Fits the output to the screen, installs the new width and lands the points.
    fn finish_reflow(
        &mut self,
        walk: &Walk,
        scratch: &mut Vec<Option<Row>>,
        points: &[Point],
        anchors: &mut Anchors,
    ) -> Reflowed {
        let cursor_row = points
            .iter()
            .find(|point| point.owner == PointOwner::Cursor)
            .and_then(|point| point.dest)
            .map(|dest| dest.row);
        self.discard_blank_tail(cursor_row, walk.cap);
        self.pad_to_screen();
        let trimmed = walk.trimmed.saturating_add(self.trim_history());
        self.cols = walk.new_cols;
        self.tabs.resize(self.cols);
        self.newest_issued = self.newest;
        if self.spare.is_none() {
            self.spare = scratch.drain(..).flatten().next();
        }
        scratch.clear();
        let view_top = self.apply_points(points, anchors);
        self.settle();
        Reflowed { trimmed, view_top }
    }

    /// Drops blank rows below the cursor while the cursor row is above the screen or the output
    /// is over the scrollback `cap`: they only came from the old screen's blank tail and would
    /// push the cursor out of view or evict content.
    fn discard_blank_tail(&mut self, cursor_row: Option<RowId>, cap: u64) {
        let Some(cursor_row) = cursor_row.filter(|&row| row >= self.oldest) else {
            return;
        };
        let screen = u64::from(self.rows);
        while self.newest > cursor_row
            && (self.newest.distance(cursor_row) >= screen
                || (self.newest + 1).distance(self.oldest) > cap)
            && self
                .slots
                .get(self.slot_index(self.newest))
                .is_some_and(Option::is_none)
        {
            self.newest = self.newest - 1;
        }
    }

    /// Appends blank rows until the output fills the screen.
    fn pad_to_screen(&mut self) {
        while (self.newest + 1).distance(self.oldest) < u64::from(self.rows) {
            self.newest = self.newest + 1;
            let index = self.slot_index(self.newest);
            if let Some(slot) = self.slots.get_mut(index) {
                *slot = None;
            }
        }
    }

    /// Writes where the points landed back to their owners; returns the viewport top.
    fn apply_points(&mut self, points: &[Point], anchors: &mut Anchors) -> RowId {
        let (oldest, newest, cols) = (self.oldest, self.newest, self.cols);
        let mut view_top = oldest;
        for point in points {
            let dest = point
                .dest
                .filter(|dest| (oldest..=newest).contains(&dest.row));
            match (point.owner, dest) {
                (PointOwner::Anchor(id), Some(dest)) => {
                    let col = dest.col.min(cols.saturating_sub(1));
                    anchors.set(id, Pos { col, ..dest });
                }
                (PointOwner::Anchor(id), None) => anchors.kill(id),
                (PointOwner::Cursor, dest) => land(&mut self.cursor, dest, cols, newest),
                (PointOwner::SavedCursor, dest) => {
                    land(&mut self.saved_cursor, dest, cols, newest);
                }
                (PointOwner::ViewportTop, dest) => view_top = dest.map_or(oldest, |d| d.row),
            }
        }
        view_top
    }

    /// Clips or pads every live row to `cols` without reflow (the alternate screen): a
    /// full-screen program redraws on resize, so re-laying its grid out would only produce
    /// text it overwrites. Ids are kept.
    pub(crate) fn clip_columns(&mut self, cols: u16, anchors: &mut Anchors) {
        let seq = self.seq;
        let mut id = self.oldest;
        while id <= self.newest {
            if let Some(row) = self.slot_row_mut(id) {
                row.fit(cols, seq);
            }
            id = id + 1;
        }
        self.cols = cols;
        self.tabs.resize(cols);
        anchors.clamp_columns(self.kind.origin()..RowId(u64::MAX), cols);
        let last = cols.saturating_sub(1);
        for cursor in [&mut self.cursor, &mut self.saved_cursor] {
            cursor.pos.col = cursor.pos.col.min(last);
            // An armed cursor sat on the old last column; it stays armed only if that column
            // was clipped to the new last one.
            cursor.pending_wrap &= cursor.pos.col == last;
        }
        self.settle();
    }

    /// The screen row the host would put the cursor on after a resize to `size`, measured on
    /// the current layout (nothing is changed).
    ///
    /// With the same width this is the cursor's screen index. Otherwise the rows from the top
    /// of the screen to the cursor row are laid out at the new width, the top row starting a
    /// line even when the row above wrapped into it, and the line holding the cursor ending at
    /// the cursor row.
    pub(crate) fn host_cursor_row(&self, size: Size) -> u16 {
        let last = size.rows.saturating_sub(1);
        if size.cols == self.cols {
            return self.cursor_row_index().min(last);
        }
        let top = self.screen_top();
        let rows_above = usize::from(self.cursor_row_index()) + 1;
        let cursor_col = if self.cursor.pending_wrap {
            usize::from(self.cols)
        } else {
            usize::from(self.cursor.pos.col)
        };
        let (mut output_rows, mut cursor_output_row) = (0usize, 0usize);
        let mut first = 0usize;
        while first < rows_above {
            let mut end = first;
            while end + 1 < rows_above && self.row(top + end as u64).wrapped() {
                end += 1;
            }
            let count = end - first + 1;
            let first_id = top + first as u64;
            let row_at = |index: usize| self.row(first_id + index as u64);
            let mut length = line_length(&row_at, count);
            // The cursor is on the last row of the last line.
            let cursor_offset = (end + 1 == rows_above).then(|| {
                let before: usize = (0..count - 1).map(|i| contribution(&row_at(i))).sum();
                before + cursor_col
            });
            if let Some(offset) = cursor_offset {
                length = length.max(offset);
            }
            let mut reader = Reader::new(&row_at, count);
            let mut line_rows = 0usize;
            let mut found = None;
            lay_out(&mut reader, length, size.cols, |span| {
                let holds_cursor =
                    cursor_offset.is_some_and(|offset| !span.wrapped || offset < span.end);
                if found.is_none() && holds_cursor {
                    found = Some(line_rows);
                }
                line_rows += 1;
            });
            if let Some(row) = found {
                cursor_output_row = output_rows + row;
            }
            output_rows += line_rows;
            first = end + 1;
        }
        u16::try_from(cursor_output_row)
            .unwrap_or(u16::MAX)
            .min(last)
    }
}

/// The number of rows of the logical line that starts at `rows[0]`.
fn line_rows(rows: &[Option<Row>]) -> usize {
    let wrapped = |row: &Option<Row>| {
        row.as_ref()
            .is_some_and(|row| row.header().flags.contains(RowFlags::WRAPPED))
    };
    let continued = rows.iter().take_while(|row| wrapped(row)).count();
    // The row that ends the line is the first one without `WRAPPED`, or the last row.
    (continued + 1).min(rows.len())
}

#[cfg(test)]
#[path = "reflow_tests.rs"]
mod reflow_tests;
