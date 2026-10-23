//! Random content, selections and grid operations: ranges stay well formed, copied text
//! matches an independent reading of the cells, and scrolling into history changes nothing.

use proptest::prelude::*;

use super::*;
use crate::ResizePolicy;
use crate::cell::CellContent;
use crate::grid::grid_fixture::{Fixture, fixture_with};
use crate::grid::{PrintMode, ScrollRegion};

const ROWS: u16 = 6;
const COLS: u16 = 8;
const SCROLLBACK: u32 = 64;
const ESC: &str = DEFAULT_SEMANTIC_ESCAPE_CHARS;
const KINDS: [SelectionKind; 4] = [
    SelectionKind::Simple,
    SelectionKind::Block,
    SelectionKind::Lines,
    SelectionKind::Semantic,
];

/// One end of a selection, to be placed on a live row of the grid under test.
#[derive(Clone, Copy, Debug)]
struct EndSpec {
    row_choice: u64,
    col: u16,
    right: bool,
}

fn arb_end() -> impl Strategy<Value = EndSpec> {
    (0..1000u64, 0..COLS, any::<bool>()).prop_map(|(row_choice, col, right)| EndSpec {
        row_choice,
        col,
        right,
    })
}

/// Lines of random letters; the screen wraps the long ones.
fn arb_lines() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec("[a-z]{0,20}", 4..12)
}

/// Lines whose length is a multiple of the width: every row they fill is full, so no cell
/// in play is blank.
fn arb_dense_lines() -> impl Strategy<Value = Vec<String>> {
    let line = (1..=3usize).prop_flat_map(|rows| {
        prop::collection::vec(b'a'..=b'z', rows * usize::from(COLS))
            .prop_map(|bytes| bytes.into_iter().map(char::from).collect::<String>())
    });
    // At least `ROWS` lines, so every row of the screen is written.
    prop::collection::vec(line, usize::from(ROWS)..10)
}

fn write_lines(f: &mut Fixture, lines: &[String]) {
    let Fixture { grid, interner } = f;
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            grid.screen_mut().carriage_return();
            grid.linefeed();
        }
        for c in line.chars() {
            grid.print(c, 1, PrintMode::default(), interner);
        }
    }
    f.check();
}

fn place(f: &Fixture, spec: EndSpec) -> (Pos, Side) {
    let screen = f.grid.screen();
    let live = screen.newest().distance(screen.oldest()) + 1;
    let pos = Pos {
        row: screen.oldest() + spec.row_choice % live,
        col: spec.col,
    };
    (pos, if spec.right { Side::Right } else { Side::Left })
}

fn select(f: &mut Fixture, kind: SelectionKind, from: (Pos, Side), to: (Pos, Side)) -> Selection {
    let mut selection = Selection::new(&mut f.grid, kind, from.0, from.1);
    selection.update(&mut f.grid, to.0, to.1);
    selection
}

/// The cells of a `Simple` range read straight from the grid: each scalar, and a newline
/// after an unwrapped row unless the range ends there.
fn read_cells(f: &Fixture, range: SelectionRange) -> String {
    let screen = f.grid.screen();
    let mut out = String::new();
    let mut pos = range.start;
    loop {
        if let CellContent::Scalar(c) = screen.row(pos.row).cell(pos.col).content() {
            out.push(c);
        }
        if pos == range.end {
            return out;
        }
        if pos.col + 1 == COLS {
            if !screen.row(pos.row).wrapped() {
                out.push('\n');
            }
            pos = Pos {
                row: pos.row + 1,
                col: 0,
            };
        } else {
            pos.col += 1;
        }
    }
}

fn assert_well_formed(f: &Fixture, range: SelectionRange) -> Result<(), TestCaseError> {
    let screen = f.grid.screen_of(range.start.row);
    prop_assert!(range.start <= range.end);
    prop_assert!(screen.oldest() <= range.start.row && range.end.row <= screen.newest());
    prop_assert!(range.start.col < screen.cols() && range.end.col < screen.cols());
    if range.is_block {
        prop_assert!(range.start.col <= range.end.col);
    }
    Ok(())
}

proptest! {
    #[test]
    fn simple_text_is_grid_slice(lines in arb_dense_lines(), a in arb_end(), b in arb_end()) {
        let mut f = fixture_with(ROWS, COLS, SCROLLBACK);
        write_lines(&mut f, &lines);
        let (from, to) = (place(&f, a), place(&f, b));
        let selection = select(&mut f, SelectionKind::Simple, from, to);
        match selection.to_range(&f.grid, ESC) {
            Some(range) => {
                assert_well_formed(&f, range)?;
                prop_assert!(!range.is_block);
                let text = selection.text(&f.grid, &f.interner.graphemes, ESC);
                prop_assert_eq!(text, Some(read_cells(&f, range)));
            }
            None => {
                // Only two ends with no whole cell between them select nothing.
                let (lo, hi) = if from.0 <= to.0 { (from, to) } else { (to, from) };
                let same_end = lo == hi;
                let touching_halves = lo.0.row == hi.0.row
                    && lo.1 == Side::Right
                    && hi.1 == Side::Left
                    && lo.0.col + 1 == hi.0.col;
                prop_assert!(same_end || touching_halves, "{from:?} {to:?}");
            }
        }
        f.check();
    }

    #[test]
    fn survives_scroll(
        lines in arb_lines(),
        a in arb_end(),
        b in arb_end(),
        kind in 0..KINDS.len(),
        n in 1..20u32,
    ) {
        let mut f = fixture_with(ROWS, COLS, SCROLLBACK);
        write_lines(&mut f, &lines);
        let (from, to) = (place(&f, a), place(&f, b));
        let selection = select(&mut f, KINDS[kind], from, to);
        let before = selection.to_range(&f.grid, ESC);
        prop_assume!(before.is_some());
        let text_before = selection.text(&f.grid, &f.interner.graphemes, ESC);
        prop_assume!(f.grid.screen().history_len() + n <= SCROLLBACK);
        f.grid.screen_mut().goto(ROWS - 1, 0);
        for _ in 0..n {
            f.grid.linefeed();
        }
        f.check();
        prop_assert_eq!(selection.to_range(&f.grid, ESC), before);
        prop_assert_eq!(selection.text(&f.grid, &f.interner.graphemes, ESC), text_before);
    }
}

#[derive(Clone, Copy, Debug)]
enum Op {
    ScrollUp { top: u16, bottom: u16, n: u16 },
    ScrollDown { top: u16, bottom: u16, n: u16 },
    InsertLines { index: u16, n: u16 },
    DeleteLines { index: u16, n: u16 },
    Linefeed,
    Resize { rows: u16, cols: u16 },
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        2 => (0..ROWS, 0..=ROWS + 1, 1..4u16)
            .prop_map(|(top, bottom, n)| Op::ScrollUp { top, bottom, n }),
        2 => (0..ROWS, 0..=ROWS + 1, 1..4u16)
            .prop_map(|(top, bottom, n)| Op::ScrollDown { top, bottom, n }),
        2 => (0..ROWS, 1..4u16).prop_map(|(index, n)| Op::InsertLines { index, n }),
        2 => (0..ROWS, 1..4u16).prop_map(|(index, n)| Op::DeleteLines { index, n }),
        2 => Just(Op::Linefeed),
        2 => (4..10u16, 4..12u16).prop_map(|(rows, cols)| Op::Resize { rows, cols }),
    ]
}

fn apply(f: &mut Fixture, op: Op) {
    match op {
        Op::ScrollUp { top, bottom, n } => {
            f.grid.scroll_up(ScrollRegion { top, bottom }, n);
        }
        Op::ScrollDown { top, bottom, n } => {
            f.grid.scroll_down(ScrollRegion { top, bottom }, n);
        }
        Op::InsertLines { index, n } => {
            f.grid.screen_mut().goto(index, 0);
            f.grid.insert_lines(n);
        }
        Op::DeleteLines { index, n } => {
            f.grid.screen_mut().goto(index, 0);
            f.grid.delete_lines(n);
        }
        Op::Linefeed => {
            f.grid.linefeed();
        }
        Op::Resize { rows, cols } => {
            f.grid
                .resize(crate::Size { rows, cols }, ResizePolicy::BottomAnchor);
        }
    }
    f.check();
}

proptest! {
    #[test]
    fn well_formed_after_ops(
        lines in arb_lines(),
        a in arb_end(),
        b in arb_end(),
        kind in 0..KINDS.len(),
        ops in prop::collection::vec(arb_op(), 1..8),
    ) {
        let mut f = fixture_with(ROWS, COLS, SCROLLBACK);
        write_lines(&mut f, &lines);
        let (from, to) = (place(&f, a), place(&f, b));
        let selection = select(&mut f, KINDS[kind], from, to);
        for op in ops {
            apply(&mut f, op);
            if let Some(range) = selection.to_range(&f.grid, ESC) {
                assert_well_formed(&f, range)?;
            }
            let text = selection.text(&f.grid, &f.interner.graphemes, ESC);
            prop_assert_eq!(text.is_some(), selection.to_range(&f.grid, ESC).is_some());
        }
        f.check();
    }
}
