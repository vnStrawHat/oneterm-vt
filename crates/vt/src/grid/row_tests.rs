use super::*;
use crate::cell::Style;
use crate::grid::integrity::IntegrityViolation as Violation;
use crate::intern::GraphemeId;

const SEQ: SeqNo = SeqNo(5);
const ID: RowId = RowId(7);

fn narrow(c: char) -> Cell {
    Cell::EMPTY.with_content(CellContent::Scalar(c))
}

fn wide(c: char) -> Cell {
    narrow(c).with_width(CellWidth::Wide)
}

/// A row of `cols` blanks with `text` written from column 0, one narrow cell per char.
fn row_with(cols: u16, text: &str) -> Row {
    let mut row = Row::new(ID, cols, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    for (col, c) in text.chars().enumerate() {
        view.set(col as u16, narrow(c));
    }
    row
}

/// The row as text; spacers show as `_`.
fn text(row: &Row) -> String {
    row.cells()
        .iter()
        .map(|cell| {
            if cell.width().is_spacer() {
                '_'
            } else {
                cell.text_char(&Default::default())
            }
        })
        .collect()
}

fn widths(row: &Row) -> Vec<CellWidth> {
    row.cells().iter().map(|cell| cell.width()).collect()
}

#[test]
fn new_row_is_template_dirty_occ_zero() {
    let mut interner = Interner::default();
    let styled = Cell::EMPTY.with_style(interner.style(Style {
        attrs: crate::cell::Attrs::BOLD,
        ..Style::DEFAULT
    }));
    let row = Row::new(ID, 4, SEQ, styled);
    assert_eq!(row.cells(), &[styled; 4]);
    assert_eq!(row.header().occ, 0);
    assert_eq!(row.header().flags, RowFlags::DIRTY | RowFlags::STYLED);
    assert_eq!((row.id(), row.header().seq), (ID, SEQ));
    assert!(row.bytes() >= 4 * size_of::<Cell>());
}

#[test]
fn set_tracks_occ_and_hints() {
    let mut row = Row::new(ID, 10, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(4, narrow('a').with_style(StyleId(3)));
    assert_eq!(view.row.header.occ, 5);
    assert!(view.row.header.flags.contains(RowFlags::STYLED));
    view.set(
        1,
        narrow('a').with_content(CellContent::Grapheme(GraphemeId(2))),
    );
    assert!(view.row.header.flags.contains(RowFlags::HAS_GRAPHEME));
    assert_eq!(view.row.header.occ, 5);
    view.set(0, narrow('a').with_extras(ExtrasId(1)));
    assert!(view.row.header.flags.contains(RowFlags::HAS_EXTRAS));
    assert_eq!(row.header.seq, SEQ);
}

#[test]
fn set_ignores_a_column_past_the_row() {
    let mut row = row_with(4, "abcd");
    let before = row.clone();
    RowMut::new(&mut row, SEQ).set(4, narrow('z'));
    assert_eq!(row.cells(), before.cells());
}

#[test]
fn set_wide_places_its_spacer() {
    let mut row = Row::new(ID, 6, SeqNo(1), Cell::EMPTY);
    let style = Cell::EMPTY.with_style(StyleId(2));
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(2, wide('W').with_style(StyleId(2)));
    assert_eq!(view.cells()[3].width(), CellWidth::WideSpacer);
    assert_eq!(view.cells()[3].style_id(), style.style_id());
    assert_eq!(view.row.header.occ, 4);
    view.set(5, wide('X'));
    assert_eq!(view.cells()[5], blank_of(wide('X')));
}

#[test]
fn set_releases_the_overwritten_pair() {
    let mut row = Row::new(ID, 6, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(1, wide('W'));
    view.set(1, narrow('n'));
    assert_eq!(widths(&row)[..3], [CellWidth::Narrow; 3]);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(3, wide('W'));
    view.set(4, narrow('n'));
    assert_eq!(text(&row), " n  n ");
    assert_eq!(widths(&row), [CellWidth::Narrow; 6]);
}

#[test]
fn set_overwriting_a_wide_with_a_wide_shifted_by_one_releases_both() {
    let mut row = Row::new(ID, 6, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(0, wide('A'));
    view.set(2, wide('B'));
    view.set(1, wide('C'));
    assert_eq!(text(&row), " C_   ");
    assert_eq!(row.check(6, None), Ok(()));
}

#[test]
fn set_degrades_stray_spacers() {
    let mut row = Row::new(ID, 4, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(0, narrow('a').with_width(CellWidth::WideSpacer));
    view.set(1, narrow('a').with_width(CellWidth::LeadingWideSpacer));
    assert_eq!(widths(&row), [CellWidth::Narrow; 4]);
    let mut view = RowMut::new(&mut row, SEQ);
    let leading = narrow(' ').with_width(CellWidth::LeadingWideSpacer);
    view.set(3, leading);
    assert_eq!(row.cells()[3], leading);
}

#[test]
fn write_reaching_last_column_clears_wrapped() {
    let mut row = Row::new(ID, 4, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set_wrapped(true);
    view.set(2, narrow('a'));
    assert!(view.row.header.flags.contains(RowFlags::WRAPPED));
    view.repair(3, narrow('b'));
    assert!(view.row.header.flags.contains(RowFlags::WRAPPED));
    view.set(3, narrow('c'));
    assert!(!view.row.header.flags.contains(RowFlags::WRAPPED));
}

#[test]
fn reset_fast_path_is_exact() {
    let a = Cell::EMPTY.with_style(StyleId(1));
    let b = Cell::EMPTY.with_style(StyleId(2));
    let mut row = Row::new(ID, 8, SeqNo(1), a);
    for round in [a, b] {
        let mut view = RowMut::new(&mut row, SEQ);
        for col in 0..3 {
            view.set(col, narrow('x').with_style(StyleId(9)));
        }
        row.reset(round, SeqNo(6));
        assert_eq!(row.cells(), vec![round; 8]);
        assert_eq!(row.header.occ, 0);
        assert_eq!(row.header.seq, SeqNo(6));
        assert_eq!(row.check(8, None), Ok(()));
    }
}

#[test]
fn recycled_row_carries_nothing() {
    let mut spare = row_with(6, "abcdef");
    let mut view = RowMut::new(&mut spare, SEQ);
    view.set(
        0,
        narrow('x').with_style(StyleId(3)).with_extras(ExtrasId(1)),
    );
    view.set_wrapped(true);
    view.mark_graphic();
    let buffer = spare.cells().as_ptr();
    let row = Row::recycle(Some(spare), RowId(99), 6, SeqNo(8), Cell::EMPTY);
    assert_eq!(row.cells().as_ptr(), buffer);
    assert_eq!(row.cells(), &[Cell::EMPTY; 6]);
    assert_eq!(row.header().flags, RowFlags::DIRTY);
    assert_eq!(
        (row.id(), row.header().occ, row.header().seq),
        (RowId(99), 0, SeqNo(8))
    );
}

#[test]
fn delete_cells_shifts_left() {
    let mut row = row_with(10, "abcdefghij");
    RowMut::new(&mut row, SEQ).delete_cells(2, 3, Cell::EMPTY);
    assert_eq!(text(&row), "abfghij   ");
    let mut row = row_with(10, "abcdefghij");
    RowMut::new(&mut row, SEQ).delete_cells(2, 40, Cell::EMPTY);
    assert_eq!(text(&row), "ab        ");
    assert_eq!(row.header.occ, 10);
}

#[test]
fn delete_cells_orphaning_a_spacer_blanks_it() {
    let mut row = Row::new(ID, 6, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(1, wide('W'));
    view.set(4, narrow('z'));
    view.delete_cells(0, 2, Cell::EMPTY);
    assert_eq!(row.check(6, None), Ok(()));
    assert_eq!(text(&row), "  z   ");
    // Starting on a spacer splits the pair from the left.
    let mut row = Row::new(ID, 6, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(1, wide('W'));
    view.delete_cells(2, 1, Cell::EMPTY);
    assert_eq!(row.check(6, None), Ok(()));
    assert_eq!(text(&row), "      ");
}

#[test]
fn delete_cells_moves_a_leading_spacer_off_the_last_column() {
    let mut row = Row::new(ID, 4, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(3, narrow(' ').with_width(CellWidth::LeadingWideSpacer));
    view.delete_cells(0, 1, Cell::EMPTY);
    assert_eq!(row.check(4, None), Ok(()));
}

#[test]
fn insert_cells_pushes_wide_off_the_end() {
    let mut row = Row::new(ID, 6, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(4, wide('W'));
    view.insert_cells(0, 1, Cell::EMPTY);
    assert_eq!(row.check(6, None), Ok(()));
    assert_eq!(row.cells()[5], Cell::EMPTY);
    assert_eq!(row.cells()[5].width(), CellWidth::Narrow);
}

#[test]
fn insert_cells_before_a_spacer_splits_the_pair() {
    let mut row = Row::new(ID, 8, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(2, wide('W'));
    view.insert_cells(3, 2, Cell::EMPTY);
    assert_eq!(row.check(8, None), Ok(()));
    assert_eq!(text(&row), "        ");
}

#[test]
fn shift_right_opens_blank_cells() {
    let mut row = row_with(6, "abcd");
    RowMut::new(&mut row, SEQ).shift_right(1, 2);
    assert_eq!(text(&row), "a  bcd");
}

#[test]
fn counts_of_zero_and_columns_past_the_row_change_nothing() {
    let mut row = row_with(5, "abcde");
    let before = row.clone();
    let mut view = RowMut::new(&mut row, SEQ);
    view.insert_cells(1, 0, Cell::EMPTY);
    view.delete_cells(1, 0, Cell::EMPTY);
    view.insert_cells(5, 3, Cell::EMPTY);
    view.delete_cells(9, 3, Cell::EMPTY);
    view.fill(3..3, Cell::EMPTY);
    view.fill(7..9, Cell::EMPTY);
    assert_eq!(row.cells(), before.cells());
}

#[test]
fn edits_repair_pairs_at_their_edges() {
    // A range ending on a Wide takes its spacer out of the pair.
    let mut row = Row::new(ID, 6, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(2, wide('W'));
    view.fill(0..3, Cell::EMPTY);
    assert_eq!(widths(&row), [CellWidth::Narrow; 6]);
    // A range starting on a spacer takes its Wide out of the pair.
    let mut view = RowMut::new(&mut row, SEQ);
    view.set(2, wide('W'));
    view.fill(3..6, Cell::EMPTY);
    assert_eq!(widths(&row), [CellWidth::Narrow; 6]);
    assert_eq!(row.check(6, None), Ok(()));
}

#[test]
fn fill_reaching_the_last_column_clears_wrapped_and_adds_hints() {
    let mut row = Row::new(ID, 4, SeqNo(1), Cell::EMPTY);
    let mut view = RowMut::new(&mut row, SEQ);
    view.set_wrapped(true);
    view.fill(1..3, Cell::EMPTY.with_style(StyleId(1)));
    assert!(view.row.header.flags.contains(RowFlags::WRAPPED));
    view.fill(2..4, Cell::EMPTY.with_style(StyleId(1)));
    assert!(!view.row.header.flags.contains(RowFlags::WRAPPED));
    assert!(view.row.header.flags.contains(RowFlags::STYLED));
}

#[test]
fn repair_stores_as_given_and_raises_occ() {
    let mut row = Row::new(ID, 4, SeqNo(1), Cell::EMPTY);
    let tab = narrow('\t').with_style(StyleId(4));
    RowMut::new(&mut row, SEQ).repair(2, tab);
    assert_eq!(row.cells()[2], tab);
    assert_eq!(row.header.occ, 3);
    assert!(row.header.flags.contains(RowFlags::STYLED));
}

#[test]
fn mark_graphic_sets_its_hint() {
    let mut row = Row::new(ID, 2, SeqNo(1), Cell::EMPTY);
    RowMut::new(&mut row, SEQ).mark_graphic();
    assert!(row.header.flags.contains(RowFlags::HAS_GRAPHIC));
    assert!(RowFlags::CONTENT_HINTS.contains(RowFlags::HAS_GRAPHIC));
    assert!(!RowFlags::CONTENT_HINTS.intersects(RowFlags::WRAPPED | RowFlags::DIRTY));
}

#[test]
fn unwritten_row_reads_blank() {
    let blank = RowRef::blank(RowId(3), 10);
    assert!(!blank.is_allocated());
    assert_eq!(
        (blank.occ(), blank.flags(), blank.wrapped()),
        (0, RowFlags::empty(), false)
    );
    assert_eq!(blank.seq(), SeqNo(0));
    assert_eq!(blank.cells().len(), 10);
    assert_eq!(blank.cell(9999), Cell::EMPTY);
    assert_eq!(blank.id(), RowId(3));
    let row = row_with(4, "ab");
    let seen = RowRef::allocated(&row);
    assert!(seen.is_allocated());
    assert_eq!(seen.cell(1), narrow('b'));
    assert_eq!(seen.occ(), 2);
    assert_eq!(seen.seq(), SEQ);
}

#[test]
fn blank_reference_of_the_widest_row_fits_the_shared_slice() {
    assert_eq!(
        RowRef::blank(RowId(0), MAX_COLS).cells().len(),
        usize::from(MAX_COLS)
    );
}

#[test]
fn check_accepts_a_well_formed_row() {
    let mut row = row_with(8, "abc");
    RowMut::new(&mut row, SEQ).set(4, wide('W'));
    assert_eq!(row.check(8, Some(&Interner::default())), Ok(()));
}

#[test]
fn check_reports_a_row_of_the_wrong_width() {
    let row = row_with(8, "abc");
    assert_eq!(row.check(9, None), Err(Violation::RowWidth { id: ID }));
}

#[test]
fn check_reports_a_wide_without_spacer() {
    let mut row = row_with(4, "");
    row.cells[1] = wide('W');
    row.header.occ = 4;
    assert_eq!(
        row.check(4, None),
        Err(Violation::BrokenWidePair { id: ID, col: 1 })
    );
}

#[test]
fn check_reports_a_spacer_without_wide() {
    let mut row = row_with(4, "");
    row.cells[2] = narrow(' ').with_width(CellWidth::WideSpacer);
    row.header.occ = 4;
    assert_eq!(
        row.check(4, None),
        Err(Violation::BrokenWidePair { id: ID, col: 2 })
    );
}

#[test]
fn check_reports_an_inland_leading_spacer() {
    let mut row = row_with(4, "");
    row.cells[1] = narrow(' ').with_width(CellWidth::LeadingWideSpacer);
    row.header.occ = 4;
    assert_eq!(
        row.check(4, None),
        Err(Violation::LeadingSpacerInland { id: ID, col: 1 })
    );
}

#[test]
fn check_reports_occ_below_a_written_cell() {
    let mut row = row_with(4, "ab");
    row.header.occ = 1;
    assert_eq!(
        row.check(4, None),
        Err(Violation::OccTailNotUniform { id: ID })
    );
}

#[test]
fn check_reports_a_styled_cell_without_its_hint() {
    let mut row = row_with(4, "");
    row.cells[0] = narrow('a').with_style(StyleId(1));
    row.header.occ = 1;
    assert_eq!(
        row.check(4, None),
        Err(Violation::MissingHint {
            id: ID,
            flag: RowFlags::STYLED
        })
    );
}

#[test]
fn check_reports_ids_missing_from_the_interner() {
    let interner = Interner::default();
    for (cell, want) in [
        (
            narrow('a').with_style(StyleId(5)),
            Violation::UnknownStyle { id: ID, col: 0 },
        ),
        (
            narrow('a').with_extras(ExtrasId(5)),
            Violation::UnknownExtras { id: ID, col: 0 },
        ),
    ] {
        let mut row = row_with(2, "");
        row.cells[0] = cell;
        row.header.occ = 1;
        row.header.flags |= hints_of(cell);
        assert_eq!(row.check(2, Some(&interner)), Err(want));
    }
    let mut row = row_with(2, "");
    row.cells[0] = narrow('a').with_content(CellContent::Grapheme(GraphemeId(40)));
    row.header.occ = 1;
    row.header.flags |= RowFlags::HAS_GRAPHEME;
    assert_eq!(
        row.check(2, Some(&interner)),
        Err(Violation::UnknownGrapheme { id: ID, col: 0 })
    );
}
