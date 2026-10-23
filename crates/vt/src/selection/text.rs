//! The text a selection range copies.

use crate::cell::{Cell, CellContent, CellWidth};
use crate::grid::{RowRef, Screen};
use crate::intern::{GraphemeArena, GraphemeId};

use super::{SelectionKind, SelectionRange};

/// Returns the text of `range` on `screen`.
///
/// Wrapped rows join without a newline; an unwrapped row ends its line after its last non-blank
/// cell. A block copies each row's rectangle slice as its own line. `Lines` ends with one `'\n'`.
pub(super) fn selection_text(
    screen: &Screen,
    graphemes: &GraphemeArena,
    kind: SelectionKind,
    range: SelectionRange,
) -> String {
    let last_col = screen.cols().saturating_sub(1);
    let mut out = String::new();
    let mut id = range.start.row;
    while id <= range.end.row {
        let row = screen.row(id);
        if range.is_block {
            if id > range.start.row {
                out.push('\n');
            }
            push_block_piece(&mut out, screen, graphemes, row, range);
        } else {
            let first = if id == range.start.row {
                range.start.col
            } else {
                0
            };
            let last = if id == range.end.row {
                range.end.col
            } else {
                last_col
            };
            push_piece(&mut out, screen, graphemes, row, first, last);
            if id < range.end.row && !row.wrapped() {
                out.push('\n');
            }
        }
        id = id + 1;
    }
    if kind == SelectionKind::Lines {
        out.push('\n');
    }
    out
}

/// A block's slice of one row: a pair cut at the left edge is taken whole, trailing blanks go.
fn push_block_piece(
    out: &mut String,
    screen: &Screen,
    graphemes: &GraphemeArena,
    row: RowRef<'_>,
    range: SelectionRange,
) {
    let starts_on_spacer = row.cell(range.start.col).width() == CellWidth::WideSpacer;
    let first = range.start.col.saturating_sub(u16::from(starts_on_spacer));
    let mark = out.len();
    push_piece(out, screen, graphemes, row, first, range.end.col);
    // A tab survives: only `' '` is trimmed.
    while out.len() > mark && out.ends_with(' ') {
        out.pop();
    }
}

/// Appends columns `first..=last` of `row` up to its content end.
fn push_piece(
    out: &mut String,
    screen: &Screen,
    graphemes: &GraphemeArena,
    row: RowRef<'_>,
    first: u16,
    last: u16,
) {
    let stop = last.saturating_add(1).min(content_end(row));
    let mut col = first;
    while col < stop {
        let cell = row.cell(col);
        col += 1;
        if cell.width().is_spacer() {
            continue;
        }
        match cell.content() {
            CellContent::Scalar('\t') => {
                out.push('\t');
                // The blanks the tab skipped over are part of it, up to the next stop.
                while col < stop && is_blank(row.cell(col)) && !screen.tabs().is_stop(col) {
                    col += 1;
                }
            }
            CellContent::Scalar(c) => out.push(c),
            CellContent::Grapheme(cluster) => out.push_str(graphemes.resolve(cluster)),
        }
    }
}

/// Whether the cell shows nothing: a blank of any style, or a spacer.
fn is_blank(cell: Cell) -> bool {
    cell.width().is_spacer()
        || matches!(
            cell.content(),
            CellContent::Scalar(' ') | CellContent::Grapheme(GraphemeId(0))
        )
}

/// One past the last column that is text: every column of a wrapped row, else the column
/// after the last non-blank cell (0 for a blank row).
fn content_end(row: RowRef<'_>) -> u16 {
    let cells = row.cells();
    if row.wrapped() {
        return u16::try_from(cells.len()).unwrap_or(u16::MAX);
    }
    // Cells at or past `occ` equal the last cell, so a blank last cell lets the scan start there.
    let mut end = match cells.last() {
        Some(&last) if is_blank(last) => usize::from(row.occ()).min(cells.len()),
        _ => cells.len(),
    };
    while end > 0 && cells.get(end - 1).is_none_or(|&cell| is_blank(cell)) {
        end -= 1;
    }
    u16::try_from(end).unwrap_or(u16::MAX)
}
