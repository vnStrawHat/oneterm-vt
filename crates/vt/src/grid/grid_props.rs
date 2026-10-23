//! Random operation sequences against a small grid: no invariant may ever break, and every
//! row whose content changed must carry the stamp of the batch that changed it.

use std::collections::BTreeMap;

use proptest::prelude::*;

use super::*;
use crate::cell::{Cell, Color, Style};
use crate::grid::anchor::{AnchorId, AnchorKind};
use crate::grid::grid_fixture::{Fixture, fixture_with};
use crate::grid::row::RowFlags;
use crate::grid::screen::{CursorOrigin, LineClear};
use crate::grid::{Pos, RowId};

const ROWS: u16 = 6;
const COLS: u16 = 8;
const SCROLLBACK: u32 = 12;
const GLYPHS: [char; 5] = ['a', ' ', '\u{FF21}', '\u{301}', 'Z'];

#[derive(Clone, Debug)]
enum Op {
    Print {
        glyph: usize,
        mode: PrintMode,
    },
    PrintAt {
        index: u16,
        col: u16,
        glyph: usize,
        mode: PrintMode,
    },
    Linefeed,
    ReverseIndex,
    Wrapline,
    ScrollUp {
        top: u16,
        bottom: u16,
        n: u16,
    },
    ScrollDown {
        top: u16,
        bottom: u16,
        n: u16,
    },
    InsertLines(u16),
    DeleteLines(u16),
    EraseLine(u8),
    EraseChars(u16),
    DeleteChars(u16),
    InsertBlanks(u16),
    EraseDisplay(u8),
    Goto {
        index: u16,
        col: u16,
    },
    SetRegion {
        top: u16,
        bottom: u16,
    },
    Tab(u16),
    ScrollViewport(i32),
    SetPen(u8),
    EnterAlt,
    LeaveAlt,
    Wipe,
    Mark(u16),
    Release(usize),
    Reset,
}

impl Op {
    /// Operations that may move anchors of the screen that is not active.
    fn switches_screens(&self) -> bool {
        matches!(
            self,
            Op::EnterAlt | Op::LeaveAlt | Op::Reset | Op::Release(_)
        )
    }
}

fn arb_mode() -> impl Strategy<Value = PrintMode> {
    (any::<bool>(), any::<bool>()).prop_map(|(insert, autowrap)| PrintMode { insert, autowrap })
}

fn arb_op() -> impl Strategy<Value = Op> {
    let glyph = 0..GLYPHS.len();
    let index = 0..ROWS + 2;
    let col = 0..COLS + 2;
    let count = 0..12u16;
    prop_oneof![
        8 => (glyph.clone(), arb_mode()).prop_map(|(glyph, mode)| Op::Print { glyph, mode }),
        4 => (index.clone(), col.clone(), glyph, arb_mode())
            .prop_map(|(index, col, glyph, mode)| Op::PrintAt { index, col, glyph, mode }),
        2 => Just(Op::Linefeed),
        1 => Just(Op::ReverseIndex),
        1 => Just(Op::Wrapline),
        2 => (0..ROWS, 0..=ROWS, 0..8u16).prop_map(|(top, bottom, n)| Op::ScrollUp { top, bottom, n }),
        2 => (0..ROWS, 0..=ROWS, 0..8u16).prop_map(|(top, bottom, n)| Op::ScrollDown { top, bottom, n }),
        1 => count.clone().prop_map(Op::InsertLines),
        1 => count.clone().prop_map(Op::DeleteLines),
        2 => (0..3u8).prop_map(Op::EraseLine),
        1 => count.clone().prop_map(Op::EraseChars),
        1 => count.clone().prop_map(Op::DeleteChars),
        1 => count.prop_map(Op::InsertBlanks),
        2 => (0..4u8).prop_map(Op::EraseDisplay),
        2 => (index, col).prop_map(|(index, col)| Op::Goto { index, col }),
        1 => (0..ROWS, 0..=ROWS + 1).prop_map(|(top, bottom)| Op::SetRegion { top, bottom }),
        1 => (0..3u16).prop_map(Op::Tab),
        1 => (-4..4i32).prop_map(Op::ScrollViewport),
        1 => (0..3u8).prop_map(Op::SetPen),
        1 => Just(Op::EnterAlt),
        1 => Just(Op::LeaveAlt),
        1 => Just(Op::Wipe),
        1 => (0..ROWS).prop_map(Op::Mark),
        1 => any::<usize>().prop_map(Op::Release),
        1 => Just(Op::Reset),
    ]
}

/// Cells and wrap state of every live row, by id.
type Snapshot = BTreeMap<RowId, (Vec<Cell>, bool)>;

fn snapshot(f: &Fixture) -> Snapshot {
    let mut rows = Snapshot::new();
    for screen in [f.grid.primary(), f.grid.alt()] {
        let mut id = screen.oldest();
        while id <= screen.newest() {
            let row = screen.row(id);
            rows.insert(id, (row.cells().to_vec(), row.wrapped()));
            id = id + 1;
        }
    }
    rows
}

fn apply(f: &mut Fixture, op: &Op, marks: &mut Vec<AnchorId>) {
    let Fixture { grid, interner } = f;
    match *op {
        Op::Print { glyph, mode } => {
            let c = GLYPHS[glyph];
            grid.print(c, crate::scalar_width(c), mode, interner);
        }
        Op::PrintAt {
            index,
            col,
            glyph,
            mode,
        } => {
            grid.screen_mut().goto(index, col);
            let c = GLYPHS[glyph];
            grid.print(c, crate::scalar_width(c), mode, interner);
        }
        Op::Linefeed => grid.linefeed(),
        Op::ReverseIndex => grid.reverse_index(),
        Op::Wrapline => grid.wrapline(),
        Op::ScrollUp { top, bottom, n } => {
            grid.scroll_up(ScrollRegion { top, bottom }, n);
        }
        Op::ScrollDown { top, bottom, n } => {
            grid.scroll_down(ScrollRegion { top, bottom }, n);
        }
        Op::InsertLines(n) => {
            grid.insert_lines(n);
        }
        Op::DeleteLines(n) => {
            grid.delete_lines(n);
        }
        Op::EraseLine(mode) => {
            let mode = [LineClear::Right, LineClear::Left, LineClear::All][usize::from(mode)];
            grid.screen_mut().erase_line(mode);
        }
        Op::EraseChars(n) => grid.screen_mut().erase_chars(n),
        Op::DeleteChars(n) => grid.screen_mut().delete_chars(n),
        Op::InsertBlanks(n) => grid.screen_mut().insert_blanks(n),
        Op::EraseDisplay(mode) => {
            let mode = [
                DisplayClear::Below,
                DisplayClear::Above,
                DisplayClear::All,
                DisplayClear::Saved,
            ][usize::from(mode)];
            grid.erase_display(mode, interner);
        }
        Op::Goto { index, col } => grid.screen_mut().goto(index, col),
        Op::SetRegion { top, bottom } => {
            grid.screen_mut()
                .set_region(top, bottom, CursorOrigin::Screen);
        }
        Op::Tab(count) => grid.put_tab(count, PrintMode::default()),
        Op::ScrollViewport(delta) => grid.screen_mut().scroll_viewport(delta),
        Op::SetPen(pen) => {
            let style = match pen {
                0 => Style::DEFAULT,
                1 => Style {
                    bg: Color::Palette(2),
                    ..Style::DEFAULT
                },
                _ => Style {
                    attrs: crate::cell::Attrs::BOLD,
                    ..Style::DEFAULT
                },
            };
            let template = Cell::EMPTY.with_style(interner.style(style));
            grid.screen_mut().set_template(template, interner);
        }
        Op::EnterAlt => grid.enter_alt(),
        Op::LeaveAlt => grid.leave_alt(),
        Op::Wipe => grid.wipe(),
        Op::Mark(index) => {
            let row = grid.screen().row_of_index(index);
            marks.push(
                grid.anchors
                    .register(AnchorKind::Mark(1), Pos { row, col: 0 }),
            );
        }
        Op::Release(pick) => {
            if !marks.is_empty() {
                let id = marks.swap_remove(pick % marks.len());
                grid.anchors.release(id);
            }
        }
        Op::Reset => grid.reset(),
    }
}

/// Marks alive before the op that sit on the screen that is not active after it.
fn inactive_marks(f: &Fixture, marks: &[AnchorId]) -> Vec<AnchorId> {
    marks
        .iter()
        .copied()
        .filter(|&id| {
            f.grid
                .anchors
                .get(id)
                .is_some_and(|pos| f.grid.alt_active() == (pos.row < RowId::ALT_ORIGIN))
        })
        .collect()
}

fn assert_changed_rows_are_stamped(f: &Fixture, before: &Snapshot, batch: SeqNo) {
    for screen in [f.grid.primary(), f.grid.alt()] {
        let blank = (vec![Cell::EMPTY; usize::from(COLS)], false);
        let mut id = screen.oldest();
        while id <= screen.newest() {
            let row = screen.row(id);
            let now = (row.cells().to_vec(), row.wrapped());
            if now != *before.get(&id).unwrap_or(&blank) {
                assert_eq!(
                    row.seq(),
                    batch,
                    "row {id:?} changed without this batch's stamp"
                );
                assert!(
                    row.flags().contains(RowFlags::DIRTY),
                    "row {id:?} not dirty"
                );
            }
            id = id + 1;
        }
    }
}

proptest! {
    #[test]
    fn random_operations_keep_every_invariant(ops in prop::collection::vec(arb_op(), 1..40)) {
        let mut f = fixture_with(ROWS, COLS, SCROLLBACK);
        let mut marks = Vec::new();
        for op in &ops {
            let batch = f.grid.begin_batch();
            let before = snapshot(&f);
            let inactive = if op.switches_screens() { Vec::new() } else { inactive_marks(&f, &marks) };
            apply(&mut f, op, &mut marks);
            f.check();
            assert_changed_rows_are_stamped(&f, &before, batch);
            for id in inactive {
                prop_assert!(f.grid.anchors.get(id).is_some(), "{op:?} killed an anchor of the hidden screen");
            }
        }
        let screen = f.grid.screen();
        prop_assert!(screen.scroll_offset() <= screen.history_len());
        prop_assert!(screen.history_len() <= SCROLLBACK);
        prop_assert!(f.grid.primary().newest() < RowId::ALT_ORIGIN);
        prop_assert!(f.grid.alt().oldest() >= RowId::ALT_ORIGIN);
    }
}
