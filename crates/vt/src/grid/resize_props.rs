//! Random sequences against resizes: growing and shrinking back must restore what the
//! rewrap promises to restore, and no sequence may reuse a row id or break an invariant.

use proptest::prelude::*;

use super::*;
use crate::cell::{Cell, CellWidth};
use crate::grid::RowId;
use crate::grid::anchor::AnchorId;
use crate::grid::grid_fixture::{Fixture, fixture_with};

const GLYPHS: [char; 5] = ['a', ' ', '\u{FF21}', '\u{301}', 'Z'];

#[derive(Clone, Debug)]
enum Op {
    Print(usize),
    Newline,
    Goto { index: u16, col: u16 },
    Wrapline,
    Mark { index: u16, col: u16 },
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        10 => (0..GLYPHS.len()).prop_map(Op::Print),
        2 => Just(Op::Newline),
        2 => (0..10u16, 0..13u16).prop_map(|(index, col)| Op::Goto { index, col }),
        1 => Just(Op::Wrapline),
        2 => (0..10u16, 0..13u16).prop_map(|(index, col)| Op::Mark { index, col }),
    ]
}

fn apply(f: &mut Fixture, op: &Op, marks: &mut Vec<AnchorId>) {
    match *op {
        Op::Print(glyph) => {
            let c = GLYPHS[glyph];
            let Fixture { grid, interner } = f;
            grid.print(c, crate::scalar_width(c), PrintMode::default(), interner);
        }
        Op::Newline => {
            f.grid.screen_mut().carriage_return();
            f.grid.linefeed();
        }
        Op::Goto { index, col } => f.grid.screen_mut().goto(index, col),
        Op::Wrapline => f.grid.wrapline(),
        Op::Mark { index, col } => {
            let col = col.min(f.grid.screen().cols() - 1);
            marks.push(f.mark(index, col));
        }
    }
    f.check();
}

/// Every logical line as its cells, spacer fillers dropped, trailing blanks trimmed.
fn canonical_lines(f: &Fixture) -> Vec<Vec<Cell>> {
    let screen = f.grid.screen();
    let mut lines = Vec::new();
    let mut line: Vec<Cell> = Vec::new();
    for id in f.live_ids() {
        let row = screen.row(id);
        line.extend(
            row.cells()
                .iter()
                .filter(|cell| cell.width() != CellWidth::LeadingWideSpacer),
        );
        if !row.wrapped() {
            lines.push(trimmed(std::mem::take(&mut line)));
        }
    }
    if !line.is_empty() {
        lines.push(trimmed(line));
    }
    lines
}

fn trimmed(mut line: Vec<Cell>) -> Vec<Cell> {
    while line.last() == Some(&Cell::EMPTY) {
        line.pop();
    }
    line
}

/// Marks that sit on a content cell the resize promises to keep, with their place in the line.
fn content_marks(
    f: &Fixture,
    marks: &[AnchorId],
    policy: ResizePolicy,
) -> Vec<(AnchorId, (usize, usize))> {
    marks
        .iter()
        .filter_map(|&id| {
            let pos = f.grid.anchors.get(id)?;
            let cell = f.grid.screen().row(pos.row).cell(pos.col);
            let content = cell != Cell::EMPTY && cell.width() != CellWidth::LeadingWideSpacer;
            // `KeepViewportTop` may cut the rows below the cursor.
            let kept =
                policy == ResizePolicy::BottomAnchor || pos.row <= f.grid.screen().cursor().pos.row;
            (content && kept).then(|| (id, f.line_offset(pos)))
        })
        .collect()
}

/// The cursor as a place in its line's text: an armed cursor sits one past the last column, which
/// is the same place as the start of a row below that continues the line. A cursor armed on an
/// already `WRAPPED` row maps to `(next row, 0, unarmed)` (cursor-anchors.md rule 2), so only
/// `offset + pending` is stable across a reflow.
fn text_position(f: &Fixture) -> (usize, usize) {
    let (line, offset, pending) = f.cursor_at();
    (line, offset + usize::from(pending))
}

/// The cursor exactly: row id, column and pending wrap.
fn exact_cursor(f: &Fixture) -> (RowId, u16, bool) {
    let cursor = f.grid.screen().cursor();
    (cursor.pos.row, cursor.pos.col, cursor.pending_wrap)
}

/// Every live row of the active screen by id: cells and wrap flag.
fn exact_rows(f: &Fixture) -> Vec<(RowId, Vec<Cell>, bool)> {
    f.live_ids()
        .into_iter()
        .map(|id| {
            let row = f.grid.screen().row(id);
            (id, row.cells().to_vec(), row.wrapped())
        })
        .collect()
}

fn arb_policy() -> impl Strategy<Value = ResizePolicy> {
    prop_oneof![
        Just(ResizePolicy::BottomAnchor),
        Just(ResizePolicy::KeepViewportTop)
    ]
}

proptest! {
    #[test]
    fn grow_then_shrink_back(
        rows in 4..8u16,
        cols in 4..12u16,
        ops in prop::collection::vec(arb_op(), 1..60),
        policy in arb_policy(),
        grow in (0..4u16, 0..8u16).prop_filter("a real change", |&(dr, dc)| dr + dc > 0),
    ) {
        let (dr, dc) = grow;
        let mut f = fixture_with(rows, cols, 64);
        let mut marks = Vec::new();
        for op in &ops {
            apply(&mut f, op, &mut marks);
        }
        let lines = canonical_lines(&f);
        let cursor = text_position(&f);
        let tracked = content_marks(&f, &marks, policy);
        let exact = (exact_rows(&f), exact_cursor(&f));
        let on_bottom_row = f.grid.screen().cursor_row_index() == rows - 1;
        let offset = f.grid.screen().scroll_offset();

        let grown = f.resize(rows + dr, cols + dc, policy);
        // A grow can leave the cursor line above the screen (its first row moved into history);
        // the cursor then clamps to the screen top by design, so the round trip cannot return it.
        prop_assume!(text_position(&f) == cursor);
        let back = f.resize(rows, cols, policy);
        prop_assume!(grown.rows_trimmed == 0 && back.rows_trimmed == 0);

        let after = canonical_lines(&f);
        let (cursor_line, cursor_offset) = cursor;
        // Rows below the cursor row can be cut by `KeepViewportTop`, so a line only counts up to
        // the cursor; the lines before it count fully.
        let before_cursor = |lines: &[Vec<Cell>]| {
            let head = lines.get(..cursor_line).unwrap_or(lines).to_vec();
            let own = lines.get(cursor_line).map(|line| trimmed(line[..line.len().min(cursor_offset)].to_vec()));
            (head, own)
        };
        prop_assert_eq!(before_cursor(&after), before_cursor(&lines));
        let through = cursor_line + 1;
        prop_assert_eq!(text_position(&f), cursor);
        for (id, place) in tracked {
            let pos = f.grid.anchors.get(id);
            prop_assert!(pos.is_some(), "a mark on a content cell died");
            prop_assert_eq!(f.line_offset(pos.unwrap_or_default()), place);
        }
        if policy == ResizePolicy::BottomAnchor {
            let trim_tail = |lines: &[Vec<Cell>]| {
                let mut end = lines.len();
                while end > through && lines[end - 1].is_empty() {
                    end -= 1;
                }
                lines[..end].to_vec()
            };
            prop_assert_eq!(trim_tail(&after), trim_tail(&lines));
        }
        if dc == 0 && (policy == ResizePolicy::KeepViewportTop || on_bottom_row) {
            prop_assert_eq!((exact_rows(&f), exact_cursor(&f)), exact);
            if offset == 0 {
                prop_assert_eq!(f.grid.screen().scroll_offset(), 0);
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Step {
    Edit(Op),
    EnterAlt,
    LeaveAlt,
    ScrollBack(i32),
    Resize {
        rows: u16,
        cols: u16,
        policy: ResizePolicy,
    },
}

fn arb_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        12 => arb_op().prop_map(Step::Edit),
        1 => Just(Step::EnterAlt),
        1 => Just(Step::LeaveAlt),
        1 => (-4..4i32).prop_map(Step::ScrollBack),
        5 => (1..10u16, 1..12u16, arb_policy())
            .prop_map(|(rows, cols, policy)| Step::Resize { rows, cols, policy }),
    ]
}

proptest! {
    #[test]
    fn no_row_id_collision(steps in prop::collection::vec(arb_step(), 1..60)) {
        let mut f = fixture_with(5, 8, 12);
        let mut marks = Vec::new();
        // The highest primary id that was ever live.
        let mut high = f.grid.primary().newest();
        for step in &steps {
            match step {
                Step::Edit(op) => apply(&mut f, op, &mut marks),
                Step::EnterAlt => f.grid.enter_alt(),
                Step::LeaveAlt => f.grid.leave_alt(),
                Step::ScrollBack(delta) => f.grid.screen_mut().scroll_viewport(*delta),
                Step::Resize { rows, cols, policy } => {
                    let reflowed_cols = *cols != f.grid.primary().cols();
                    let outcome = f.resize(*rows, *cols, *policy);
                    prop_assert_eq!(outcome.reflowed, reflowed_cols);
                    if outcome.reflowed {
                        // Fresh ids sit above every id that ever was live, released ones too.
                        prop_assert!(f.grid.primary().oldest() > high);
                    }
                }
            }
            f.check();
            high = high.max(f.grid.primary().newest());
            let screen = f.grid.primary();
            prop_assert!(screen.newest() < RowId::ALT_ORIGIN);
            prop_assert!(f.grid.alt().oldest() >= RowId::ALT_ORIGIN);
            prop_assert!(screen.scroll_offset() <= screen.history_len());
            for (_, anchor) in f.grid.anchors.iter() {
                if anchor.alive && !anchor.kind.is_screen_owned() {
                    let lane = f.grid.screen_of(anchor.pos.row);
                    prop_assert!(lane.row_range().contains(&anchor.pos.row));
                    prop_assert!(anchor.pos.col < lane.cols());
                }
            }
        }
    }
}
