//! Word, bracket and line searches over one screen's live rows.
//!
//! Everything here reads cells through `Screen::row`; nothing allocates or touches the
//! interner. Every scan is capped at a budget (cells, or rows for line walks) per direction, so
//! one hostile wrapped line cannot make a snapshot cost O(history).

use crate::cell::CellContent;
use crate::grid::{Pos, Screen};

/// Most cells (rows for line walks) a scan visits per direction.
pub(super) const EXPAND_BUDGET: u32 = 65_536;

/// The bracket pairs a coincident semantic selection matches.
const BRACKET_PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// The position after `pos` in stream order; `None` past the newest row's last column.
pub(super) fn next(screen: &Screen, pos: Pos) -> Option<Pos> {
    if pos.col.saturating_add(1) < screen.cols() {
        return Some(Pos {
            row: pos.row,
            col: pos.col + 1,
        });
    }
    (pos.row < screen.newest()).then(|| Pos {
        row: pos.row + 1,
        col: 0,
    })
}

/// The position before `pos` in stream order; `None` before the oldest row's first column.
pub(super) fn prev(screen: &Screen, pos: Pos) -> Option<Pos> {
    if pos.col > 0 {
        return Some(Pos {
            row: pos.row,
            col: pos.col - 1,
        });
    }
    (pos.row > screen.oldest()).then(|| Pos {
        row: pos.row - 1,
        col: screen.cols().saturating_sub(1),
    })
}

/// Whether the cell at `pos` is a word delimiter. A spacer or a cluster never is: the set
/// holds lone scalars, and resolving a cluster would need the interner.
fn is_escape(screen: &Screen, pos: Pos, escape_chars: &str) -> bool {
    let cell = screen.row(pos.row).cell(pos.col);
    if cell.width().is_spacer() {
        return false;
    }
    matches!(cell.content(), CellContent::Scalar(c) if escape_chars.contains(c))
}

/// The first cell of the word containing `pos`. The cell at `pos` itself is not tested.
pub(super) fn word_start(screen: &Screen, pos: Pos, escape_chars: &str, budget: u32) -> Pos {
    let mut pos = pos;
    for _ in 0..budget {
        let Some(before) = prev(screen, pos) else {
            break;
        };
        let crossed_hard_break = before.row != pos.row && !screen.row(before.row).wrapped();
        if crossed_hard_break || is_escape(screen, before, escape_chars) {
            break;
        }
        pos = before;
    }
    pos
}

/// The last cell of the word containing `pos`. The cell at `pos` itself is not tested.
pub(super) fn word_end(screen: &Screen, pos: Pos, escape_chars: &str, budget: u32) -> Pos {
    let last_col = screen.cols().saturating_sub(1);
    let mut pos = pos;
    for _ in 0..budget {
        if pos.col == last_col && !screen.row(pos.row).wrapped() {
            break;
        }
        let Some(after) = next(screen, pos) else {
            break;
        };
        if is_escape(screen, after, escape_chars) {
            break;
        }
        pos = after;
    }
    pos
}

/// The position of the bracket matching the one at `pos`, nesting counted; `None` if the
/// cell is no bracket, the match is not within the budget or there is none.
pub(super) fn bracket_match(screen: &Screen, pos: Pos, budget: u32) -> Option<Pos> {
    let cell = screen.row(pos.row).cell(pos.col);
    if cell.width().is_spacer() {
        return None;
    }
    let CellContent::Scalar(bracket) = cell.content() else {
        return None;
    };
    let (partner, forward) = BRACKET_PAIRS.iter().find_map(|&(open, close)| {
        if bracket == open {
            Some((close, true))
        } else if bracket == close {
            Some((open, false))
        } else {
            None
        }
    })?;
    let mut depth = 0_u32;
    let mut at = pos;
    for _ in 0..budget {
        at = if forward {
            next(screen, at)?
        } else {
            prev(screen, at)?
        };
        match screen.row(at.row).cell(at.col).content() {
            CellContent::Scalar(c) if c == bracket => depth += 1,
            CellContent::Scalar(c) if c == partner => {
                if depth == 0 {
                    return Some(at);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// The first cell of the logical line holding `pos`: column 0 of its first row.
pub(super) fn line_start(screen: &Screen, pos: Pos, budget: u32) -> Pos {
    let mut row = pos.row;
    for _ in 0..budget {
        if row > screen.oldest() && screen.row(row - 1).wrapped() {
            row = row - 1;
        } else {
            break;
        }
    }
    Pos { row, col: 0 }
}

/// The last cell of the logical line holding `pos`: the last column of its last row.
pub(super) fn line_end(screen: &Screen, pos: Pos, budget: u32) -> Pos {
    let mut row = pos.row;
    for _ in 0..budget {
        if row < screen.newest() && screen.row(row).wrapped() {
            row = row + 1;
        } else {
            break;
        }
    }
    Pos {
        row,
        col: screen.cols().saturating_sub(1),
    }
}
