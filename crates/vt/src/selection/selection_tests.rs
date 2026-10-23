use super::expand::{self, EXPAND_BUDGET};
use super::*;
use crate::ResizePolicy;
use crate::cell::{Cell, CellContent, CellWidth, Color, Style};
use crate::grid::grid_fixture::{Fixture, fixture, fixture_with};
use crate::grid::{PrintMode, RowId, ScrollRegion};

use Side::{Left, Right};

const ESC: &str = DEFAULT_SEMANTIC_ESCAPE_CHARS;

/// Test helpers as methods, so arguments may read the fixture while it is borrowed mutably.
trait SelectionFixture {
    /// The position at screen index `index` (negative: history above the screen) and `col`.
    fn at(&self, index: i32, col: u16) -> Pos;
    fn select(&mut self, kind: SelectionKind, from: (Pos, Side), to: (Pos, Side)) -> Selection;
    /// `Left` at `from` to `Right` at `to`: every cell from `from` through `to`.
    fn span(&mut self, kind: SelectionKind, from: Pos, to: Pos) -> Selection;
    fn range(&self, selection: &Selection) -> Option<SelectionRange>;
    fn text(&self, selection: &Selection) -> Option<String>;
}

impl SelectionFixture for Fixture {
    fn at(&self, index: i32, col: u16) -> Pos {
        let top = self.grid.screen().screen_top();
        let row = if index >= 0 {
            top + u64::from(index.unsigned_abs())
        } else {
            top - u64::from(index.unsigned_abs())
        };
        Pos { row, col }
    }

    fn select(&mut self, kind: SelectionKind, from: (Pos, Side), to: (Pos, Side)) -> Selection {
        let mut selection = Selection::new(&mut self.grid, kind, from.0, from.1);
        selection.update(&mut self.grid, to.0, to.1);
        selection
    }

    fn span(&mut self, kind: SelectionKind, from: Pos, to: Pos) -> Selection {
        self.select(kind, (from, Left), (to, Right))
    }

    fn range(&self, selection: &Selection) -> Option<SelectionRange> {
        selection.to_range(&self.grid, ESC)
    }

    fn text(&self, selection: &Selection) -> Option<String> {
        selection.text(&self.grid, &self.interner.graphemes, ESC)
    }
}

/// Prints `lines` one per row, a hard line break between them.
fn write_lines(f: &mut Fixture, lines: &[&str]) {
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            f.newline();
        }
        f.print(line);
    }
}

/// The text of a `Simple` span of `a..=b` on screen row 0 of a fresh single-row grid.
fn word_at(content: &str, escape_chars: &str, col: u16) -> String {
    let mut f = fixture(5, 20);
    f.print(content);
    let pos = f.at(0, col);
    let selection = f.select(SelectionKind::Semantic, (pos, Left), (pos, Left));
    let text = selection.text(&f.grid, &f.interner.graphemes, escape_chars);
    f.check();
    text.unwrap_or_default()
}

// simple

#[test]
fn side_rules() {
    let mut f = fixture(5, 10);
    f.print("abcdefghij");
    let (p2, p5) = (f.at(0, 2), f.at(0, 5));
    for (sides, expected) in [
        ((Left, Right), "cdef"),
        ((Right, Left), "de"),
        ((Left, Left), "cde"),
        ((Right, Right), "def"),
    ] {
        let selection = f.select(SelectionKind::Simple, (p2, sides.0), (p5, sides.1));
        assert_eq!(f.text(&selection).as_deref(), Some(expected));
        selection.release(&mut f.grid);
    }
    f.check();
}

#[test]
fn empty_when_no_whole_cell() {
    let mut f = fixture(5, 10);
    f.print("abcdefghij");
    let (p3, p4) = (f.at(0, 3), f.at(0, 4));
    let cases = [
        ((p3, Left), (p3, Left), None),
        ((p3, Right), (p3, Right), None),
        ((p3, Right), (p4, Left), None),
        ((p4, Left), (p3, Right), None),
        ((p3, Left), (p3, Right), Some("d")),
        ((p3, Right), (p3, Left), Some("d")),
    ];
    for (from, to, expected) in cases {
        let selection = f.select(SelectionKind::Simple, from, to);
        assert_eq!(f.text(&selection).as_deref(), expected, "{from:?} {to:?}");
        assert_eq!(f.range(&selection).is_some(), expected.is_some());
    }
    f.check();
}

#[test]
fn row_edge_half_cells_keep_last_cell() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abcdefghij", "klmnopqrst"]);
    let from = (f.at(0, 9), Right);
    let to = (f.at(1, 0), Left);
    let selection = f.select(SelectionKind::Simple, from, to);
    let expected = SelectionRange {
        start: f.at(0, 9),
        end: f.at(0, 9),
        is_block: false,
    };
    assert_eq!(f.range(&selection), Some(expected));
    assert_eq!(f.text(&selection).as_deref(), Some("j"));
    f.check();
}

#[test]
fn reversed_ends_same_range_every_kind() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["foo(bar) b", "中文 abcde", "klmnopqrst"]);
    let mut points = Vec::new();
    for (index, col) in [(0, 0), (0, 4), (0, 9), (1, 1), (1, 6), (2, 3), (2, 9)] {
        for side in [Left, Right] {
            points.push((f.at(index, col), side));
        }
    }
    for kind in [
        SelectionKind::Simple,
        SelectionKind::Block,
        SelectionKind::Lines,
        SelectionKind::Semantic,
    ] {
        for &from in &points {
            for &to in &points {
                let forward = f.select(kind, from, to);
                let backward = f.select(kind, to, from);
                assert_eq!(
                    f.range(&forward),
                    f.range(&backward),
                    "{kind:?} {from:?} {to:?}"
                );
                forward.release(&mut f.grid);
                backward.release(&mut f.grid);
            }
        }
    }
    f.check();
}

// block

#[test]
fn block_columns_from_sides() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abcdefghij", "klmnopqrst", "uvwxyzabcd"]);
    // Top right to bottom left normalises to the columns between the two pointer halves.
    let selection = f.span(SelectionKind::Block, f.at(0, 6), f.at(2, 2));
    let expected = SelectionRange {
        start: f.at(0, 3),
        end: f.at(2, 5),
        is_block: true,
    };
    assert_eq!(f.range(&selection), Some(expected));
    // Left half of the bottom left to right half of the top right keeps both corner columns.
    let selection = f.span(SelectionKind::Block, f.at(2, 2), f.at(0, 6));
    let corners = SelectionRange {
        start: f.at(0, 2),
        end: f.at(2, 6),
        is_block: true,
    };
    assert_eq!(f.range(&selection), Some(corners));
    // Swapping the two pointer events of the first gesture changes nothing.
    let selection = f.select(
        SelectionKind::Block,
        (f.at(2, 2), Right),
        (f.at(0, 6), Left),
    );
    assert_eq!(f.range(&selection), Some(expected));
    // Top left to bottom right keeps both corner columns.
    let selection = f.span(SelectionKind::Block, f.at(0, 2), f.at(2, 6));
    let expected = SelectionRange {
        start: f.at(0, 2),
        end: f.at(2, 6),
        is_block: true,
    };
    assert_eq!(f.range(&selection), Some(expected));
    f.check();
}

#[test]
fn block_one_column_and_empty() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abcdefghij", "klmnopqrst", "uvwxyzabcd"]);
    let (c, next) = (4, 5);
    // Touching halves on either row: no whole column.
    let touching = f.select(
        SelectionKind::Block,
        (f.at(0, c), Right),
        (f.at(2, next), Left),
    );
    assert_eq!(f.range(&touching), None);
    // Left half to right half of the same column: that column, on both rows.
    let column = f.select(
        SelectionKind::Block,
        (f.at(0, c), Left),
        (f.at(1, c), Right),
    );
    let expected = SelectionRange {
        start: f.at(0, c),
        end: f.at(1, c),
        is_block: true,
    };
    assert_eq!(f.range(&column), Some(expected));
    assert_eq!(f.text(&column).as_deref(), Some("e\no"));
    // Right half to left half of the same column on two rows: the columns would cross.
    let crossed = f.select(
        SelectionKind::Block,
        (f.at(0, c), Right),
        (f.at(1, c), Left),
    );
    assert_eq!(f.range(&crossed), None);
    f.check();
}

#[test]
fn block_text_rectangle() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abcdefghij", "klmnopqrst", "uvwxyzabcd"]);
    let selection = f.span(SelectionKind::Block, f.at(0, 2), f.at(2, 4));
    assert_eq!(f.text(&selection).as_deref(), Some("cde\nmno\nwxy"));
    f.check();
}

#[test]
fn block_trims_each_row() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abcdefgh", "kl", "uvwxyz"]);
    let selection = f.span(SelectionKind::Block, f.at(0, 1), f.at(2, 6));
    assert_eq!(f.text(&selection).as_deref(), Some("bcdefg\nl\nvwxyz"));
    f.check();
}

#[test]
fn block_wide_edges() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["中文abc", "xy中zz"]);
    let selection = f.span(SelectionKind::Block, f.at(0, 1), f.at(1, 3));
    assert_eq!(f.text(&selection).as_deref(), Some("中文\ny中"));
    let selected = f.range(&selection).expect("a range");
    let screen = f.grid.screen();
    // The rectangle cuts the pair at (0, 0)..(0, 1) and (1, 2)..(1, 3): both halves paint.
    let first = screen.row(f.at(0, 0).row);
    assert!(!selected.contains(f.at(0, 0)));
    assert!(selected.contains_cell(first, 0));
    assert!(selected.contains_cell(first, 1));
    let second = screen.row(f.at(1, 0).row);
    assert!(selected.contains_cell(second, 2) && selected.contains_cell(second, 3));
    assert!(!selected.contains_cell(second, 0));
    f.check();
}

// semantic

#[test]
fn word_uses_escape_chars() {
    assert_eq!(word_at("foo bar-baz qux", ESC, 5), "bar-baz");
    assert_eq!(word_at("foo bar-baz qux", " -", 5), "bar");
    assert_eq!(word_at("foo bar-baz qux", "", 5), "foo bar-baz qux");
}

#[test]
fn word_stops_at_hard_line_end() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abc", "def"]);
    let pos = f.at(1, 1);
    let selection = f.select(SelectionKind::Semantic, (pos, Left), (pos, Left));
    let selected = f.range(&selection).expect("a range");
    assert_eq!((selected.start, selected.end), (f.at(1, 0), f.at(1, 2)));
    assert_eq!(f.text(&selection).as_deref(), Some("def"));
    f.check();
}

#[test]
fn semantic_click_on_spacer_skips_bracket_search() {
    let mut f = fixture(5, 10);
    f.print("(ab");
    // `RowMut::set` blanks a stray `WideSpacer`; a leading spacer in the last column is kept.
    let id = f.grid.screen().row_of_index(0);
    let spacer = Cell::EMPTY
        .with_content(CellContent::Scalar(')'))
        .with_width(CellWidth::LeadingWideSpacer);
    f.grid
        .screen_mut()
        .row_mut(id)
        .expect("live")
        .set(9, spacer);
    let pos = f.at(0, 9);
    let selection = f.select(SelectionKind::Semantic, (pos, Left), (pos, Left));
    let selected = f.range(&selection).expect("a range");
    assert_ne!(
        selected.start,
        f.at(0, 0),
        "no bracket search from a spacer"
    );
}

#[test]
fn word_crosses_wrap() {
    let mut f = fixture(5, 10);
    f.print("abcdefghijklm");
    let pos = f.at(1, 0);
    let selection = f.select(SelectionKind::Semantic, (pos, Left), (pos, Left));
    let selected = f.range(&selection).expect("a range");
    assert_eq!((selected.start, selected.end), (f.at(0, 0), f.at(1, 2)));
    assert_eq!(f.text(&selection).as_deref(), Some("abcdefghijklm"));
    f.check();
}

#[test]
fn word_over_wide_chars() {
    assert_eq!(word_at("中文 abc", ESC, 0), "中文");
    assert_eq!(word_at("中文 abc", ESC, 1), "中文");
    assert_eq!(word_at("中文 abc", ESC, 3), "中文");
    assert_eq!(word_at("中文 abc", ESC, 5), "abc");
}

#[test]
fn click_on_delimiter_takes_both_words() {
    assert_eq!(word_at("abc def", ESC, 3), "abc def");
}

#[test]
fn grapheme_cell_is_never_escape() {
    // The cluster cell would match the set if clusters were tested; they are not.
    assert_eq!(word_at("abe\u{301}cd", "e\u{301}", 3), "abe\u{301}cd");
    assert_eq!(word_at("abe\u{301}cd", "e\u{301}", 4), "abe\u{301}cd");
}

#[test]
fn bracket_match_both_directions() {
    let mut f = fixture(5, 20);
    f.print("foo(bar(baz))");
    let mut ranges = Vec::new();
    for col in [3, 12] {
        let pos = f.at(0, col);
        let selection = f.select(SelectionKind::Semantic, (pos, Left), (pos, Left));
        assert_eq!(f.text(&selection).as_deref(), Some("(bar(baz))"));
        ranges.push(f.range(&selection));
    }
    assert_eq!(ranges[0], ranges[1]);
    assert_eq!(ranges[0].map(|r| (r.start.col, r.end.col)), Some((3, 12)));
    // The inner pair matches itself, not the outer one.
    let pos = f.at(0, 7);
    let inner = f.select(SelectionKind::Semantic, (pos, Left), (pos, Left));
    assert_eq!(f.text(&inner).as_deref(), Some("(baz)"));
    f.check();
}

#[test]
fn bracket_without_match_falls_back_to_word() {
    // No partner: the click takes the word next to the bracket instead.
    assert_eq!(word_at("foo(bar baz", ESC, 3), "foo(bar");
    assert_eq!(word_at("a) b", ESC, 1), "a)");
    // A different bracket kind in between does not close it.
    assert_eq!(word_at("(a]", ESC, 0), "(a");
}

#[test]
fn bracket_across_rows() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["f(a,", "  b)"]);
    for pos in [f.at(0, 1), f.at(1, 3)] {
        let selection = f.select(SelectionKind::Semantic, (pos, Left), (pos, Left));
        assert_eq!(f.text(&selection).as_deref(), Some("(a,\n  b)"));
    }
    f.check();
}

#[test]
fn semantic_never_empty() {
    let mut f = fixture(5, 10);
    f.print("abc def");
    let pos = f.at(0, 1);
    let selection = f.select(SelectionKind::Semantic, (pos, Right), (pos, Left));
    assert_eq!(f.text(&selection).as_deref(), Some("abc"));
    f.check();
}

#[test]
fn expansion_budget_caps_scan() {
    let mut f = fixture(5, 10);
    // 25 letters: a three-row word whose first two rows are wrapped.
    f.print("abcdefghijklmnopqrstuvwxy");
    let screen = f.grid.screen();
    let p = |index, col| f.at(index, col);
    assert_eq!(expand::word_start(screen, p(2, 2), ESC, 4), p(1, 8));
    assert_eq!(expand::word_end(screen, p(0, 3), ESC, 4), p(0, 7));
    assert_eq!(expand::word_start(screen, p(2, 2), ESC, 100), p(0, 0));
    assert_eq!(expand::word_end(screen, p(0, 3), ESC, 100), p(2, 4));
    assert_eq!(expand::line_start(screen, p(2, 0), 1), p(1, 0));
    assert_eq!(expand::line_start(screen, p(2, 0), 9), p(0, 0));
    assert_eq!(expand::line_end(screen, p(0, 0), 1), p(1, 9));
    assert_eq!(expand::line_end(screen, p(0, 0), 9), p(2, 9));
    assert_eq!(EXPAND_BUDGET, 65_536);

    let mut g = fixture(5, 10);
    g.print("(abcdefghijklmnopqrstuvw)");
    let screen = g.grid.screen();
    let open = Pos {
        row: screen.oldest(),
        col: 0,
    };
    let close = Pos {
        row: screen.oldest() + 2,
        col: 4,
    };
    assert_eq!(expand::bracket_match(screen, open, 4), None);
    assert_eq!(expand::bracket_match(screen, open, 24), Some(close));
    assert_eq!(expand::bracket_match(screen, close, 24), Some(open));
    assert_eq!(expand::bracket_match(screen, close, 23), None);
}

#[test]
fn hostile_wrapped_grid_is_capped() {
    // 100 rows of 2048 cells, all one wrapped word: 204,800 cells, past the budget both ways.
    let mut f = fixture(100, 2048);
    let cols = 2048;
    let screen = f.grid.screen_mut();
    for index in 0..100 {
        let id = screen.row_of_index(index);
        if let Some(mut row) = screen.row_mut(id) {
            for col in 0..cols {
                row.set(col, Cell::EMPTY.with_content(CellContent::Scalar('a')));
            }
            row.set_wrapped(true);
        }
    }
    let click = f.at(50, 0);
    let rows_in_budget = u64::from(EXPAND_BUDGET) / u64::from(cols);
    for kind in [SelectionKind::Semantic, SelectionKind::Lines] {
        let selection = f.select(kind, (click, Left), (click, Left));
        let selected = f.range(&selection).expect("a range");
        let screen = f.grid.screen();
        match kind {
            SelectionKind::Semantic => {
                assert_eq!(selected.start.row, click.row - rows_in_budget);
                assert_eq!(selected.end.row, click.row + rows_in_budget);
            }
            _ => {
                assert_eq!(selected.start.row, screen.oldest());
                assert_eq!(selected.end.row, screen.newest());
            }
        }
    }
    f.check();
}

// lines

#[test]
fn lines_follow_wraps() {
    let mut f = fixture(5, 10);
    f.print("0123456789abcdefghijklmno");
    let pos = f.at(1, 3);
    let selection = f.select(SelectionKind::Lines, (pos, Left), (pos, Right));
    let expected = SelectionRange {
        start: f.at(0, 0),
        end: f.at(2, 9),
        is_block: false,
    };
    assert_eq!(f.range(&selection), Some(expected));
    assert_eq!(
        f.text(&selection).as_deref(),
        Some("0123456789abcdefghijklmno\n")
    );
    f.check();
}

#[test]
fn lines_never_empty() {
    let mut f = fixture(5, 10);
    f.print("abc");
    let pos = f.at(3, 5);
    let selection = f.select(SelectionKind::Lines, (pos, Right), (pos, Left));
    // A blank line is a line: just its newline.
    assert_eq!(f.text(&selection).as_deref(), Some("\n"));
    f.check();
}

#[test]
fn lines_two_logical_lines() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["l1", "l2", "l3"]);
    let selection = f.span(SelectionKind::Lines, f.at(0, 1), f.at(1, 0));
    assert_eq!(f.text(&selection).as_deref(), Some("l1\nl2\n"));
    f.check();
}

// text

#[test]
fn wrapped_rows_join() {
    let mut f = fixture(5, 10);
    f.print("0123456789abcde");
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(1, 4));
    assert_eq!(f.text(&selection).as_deref(), Some("0123456789abcde"));
    // A selection ending on the wrapped row's last cell has no newline either.
    let selection = f.span(SelectionKind::Simple, f.at(0, 5), f.at(0, 9));
    assert_eq!(f.text(&selection).as_deref(), Some("56789"));
    f.check();
}

#[test]
fn hard_break_newline_and_trim() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["hi", "there"]);
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(1, 9));
    assert_eq!(f.text(&selection).as_deref(), Some("hi\nthere"));
    // Blank rows in between keep their newlines.
    let mut g = fixture(5, 10);
    write_lines(&mut g, &["a", "", "b"]);
    let selection = g.span(SelectionKind::Simple, g.at(0, 0), g.at(2, 9));
    assert_eq!(g.text(&selection).as_deref(), Some("a\n\nb"));
    f.check();
    g.check();
}

#[test]
fn styled_blanks_trimmed() {
    let mut f = fixture(5, 10);
    f.print("ab");
    f.set_style(Style {
        bg: Color::Palette(4),
        ..Style::DEFAULT
    });
    f.print("   ");
    f.set_style(Style::DEFAULT);
    f.newline();
    f.print("cd");
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(1, 9));
    assert_eq!(f.text(&selection).as_deref(), Some("ab\ncd"));
    f.check();
}

#[test]
fn spacers_skipped_graphemes_resolved() {
    let mut f = fixture(5, 10);
    f.print("a中be\u{301}");
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(0, 9));
    assert_eq!(f.text(&selection).as_deref(), Some("a中be\u{301}"));
    f.check();
}

#[test]
fn start_on_spacer_takes_glyph() {
    let mut f = fixture(5, 10);
    f.print("a中b");
    let selection = f.span(SelectionKind::Simple, f.at(0, 2), f.at(0, 3));
    assert_eq!(f.text(&selection).as_deref(), Some("中b"));
    assert_eq!(f.range(&selection).map(|r| r.start), Some(f.at(0, 1)));
    // An end on the glyph takes its spacer too.
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(0, 1));
    assert_eq!(f.text(&selection).as_deref(), Some("a中"));
    assert_eq!(f.range(&selection).map(|r| r.end), Some(f.at(0, 2)));
    f.check();
}

#[test]
fn leading_spacer_end_takes_wrapped_glyph() {
    let mut f = fixture(5, 10);
    f.print("abcdefghi中");
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(0, 9));
    assert_eq!(f.text(&selection).as_deref(), Some("abcdefghi中"));
    assert_eq!(f.range(&selection).map(|r| r.end), Some(f.at(1, 1)));
    f.check();
}

#[test]
fn tab_run_copies_as_tab() {
    let mut f = fixture(5, 12);
    f.print("a");
    f.grid.put_tab(1, PrintMode::default());
    f.print("b");
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(0, 8));
    assert_eq!(f.text(&selection).as_deref(), Some("a\tb"));
    // Starting inside the run copies the blanks that are left as spaces.
    let selection = f.span(SelectionKind::Simple, f.at(0, 5), f.at(0, 8));
    assert_eq!(f.text(&selection).as_deref(), Some("   b"));
    // A trailing tab keeps its `'\t'` through a block's trim.
    let mut g = fixture(5, 12);
    g.print("a");
    g.grid.put_tab(1, PrintMode::default());
    let selection = g.span(SelectionKind::Block, g.at(0, 0), g.at(0, 11));
    assert_eq!(g.text(&selection).as_deref(), Some("a\t"));
    f.check();
}

#[test]
fn blanks_only_is_empty_string() {
    let mut f = fixture(5, 10);
    f.print("abc");
    let selection = f.span(SelectionKind::Simple, f.at(2, 0), f.at(3, 5));
    assert_eq!(f.text(&selection).as_deref(), Some("\n"));
    let selection = f.span(SelectionKind::Simple, f.at(0, 5), f.at(0, 8));
    assert_eq!(f.text(&selection).as_deref(), Some(""));
    f.check();
}

#[test]
fn empty_selection_text_none() {
    let mut f = fixture(5, 10);
    f.print("abc");
    let pos = f.at(0, 1);
    let selection = Selection::new(&mut f.grid, SelectionKind::Simple, pos, Left);
    assert_eq!(f.range(&selection), None);
    assert_eq!(f.text(&selection), None);
    f.check();
}

// api

#[test]
fn select_all_covers_history() {
    let mut f = fixture(3, 10);
    let lines: Vec<String> = (0..8).map(|n| format!("line{n}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    write_lines(&mut f, &refs);
    let selection = Selection::all(&mut f.grid);
    let screen = f.grid.screen();
    let expected = SelectionRange {
        start: Pos {
            row: screen.oldest(),
            col: 0,
        },
        end: Pos {
            row: screen.newest(),
            col: 9,
        },
        is_block: false,
    };
    assert_eq!(f.range(&selection), Some(expected));
    let all = f.text(&selection).expect("text");
    assert!(all.starts_with("line0\nline1\n"));
    assert!(all.ends_with("line6\nline7"));
    f.check();
}

#[test]
fn release_frees_both_anchors() {
    let mut f = fixture(3, 10);
    let before = f.grid.anchors().live();
    let start = f.at(1, 1);
    let selection = Selection::new(&mut f.grid, SelectionKind::Simple, start, Left);
    assert_eq!(f.grid.anchors().live(), before + 2);
    selection.release(&mut f.grid);
    assert_eq!(f.grid.anchors().live(), before);
    f.check();
}

#[test]
fn positions_clamp_into_lane() {
    let mut f = fixture(3, 10);
    f.print("abc");
    let screen = f.grid.screen();
    let (oldest, newest) = (screen.oldest(), screen.newest());
    let far = Pos {
        row: newest + 50,
        col: 400,
    };
    let mut selection = Selection::new(&mut f.grid, SelectionKind::Simple, far, Right);
    f.check();
    let last = Pos {
        row: newest,
        col: 9,
    };
    assert_eq!(f.grid.anchors().get(selection.start), Some(last));
    // Above the oldest row of the primary lane: the lane starts at its origin.
    selection.update(
        &mut f.grid,
        Pos {
            row: RowId(0),
            col: 3,
        },
        Left,
    );
    f.check();
    assert_eq!(
        f.grid.anchors().get(selection.end),
        Some(Pos {
            row: oldest,
            col: 3
        })
    );
    // A row in the alternate lane clamps into the alternate screen's rows.
    let alt = Pos {
        row: RowId::ALT_ORIGIN + 1000,
        col: 400,
    };
    selection.update(&mut f.grid, alt, Left);
    f.check();
    let end = f.grid.anchors().get(selection.end).expect("alive");
    assert_eq!(end.row, f.grid.alt().newest());
    assert_eq!(end.col, 9);
}

#[test]
fn ends_in_two_lanes_resolve_none() {
    let mut f = fixture(3, 10);
    let start = f.at(0, 0);
    let mut selection = Selection::new(&mut f.grid, SelectionKind::Simple, start, Left);
    let alt = Pos {
        row: RowId::ALT_ORIGIN,
        col: 2,
    };
    selection.update(&mut f.grid, alt, Right);
    for kind in [
        SelectionKind::Simple,
        SelectionKind::Block,
        SelectionKind::Semantic,
        SelectionKind::Lines,
    ] {
        selection.kind = kind;
        assert_eq!(f.range(&selection), None);
        assert_eq!(f.text(&selection), None);
    }
    f.check();
}

#[test]
fn selection_on_inactive_screen_reads_its_lane() {
    let mut f = fixture(3, 10);
    write_lines(&mut f, &["abc", "def"]);
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(1, 2));
    f.grid.enter_alt();
    f.check();
    assert!(f.grid.alt_active());
    assert_eq!(f.text(&selection).as_deref(), Some("abc\ndef"));
    f.check();
}

#[test]
fn contains_rules() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abcdefghij", "klmnopqrst", "uvwxyzabcd"]);
    let simple = SelectionRange {
        start: f.at(0, 5),
        end: f.at(2, 3),
        is_block: false,
    };
    assert!(simple.contains(f.at(0, 5)) && simple.contains(f.at(2, 3)));
    assert!(simple.contains(f.at(1, 0)) && simple.contains(f.at(1, 9)));
    assert!(!simple.contains(f.at(0, 4)) && !simple.contains(f.at(2, 4)));
    let block = SelectionRange {
        is_block: true,
        start: f.at(0, 2),
        end: f.at(2, 4),
    };
    assert!(block.contains(f.at(1, 2)) && block.contains(f.at(1, 4)));
    assert!(!block.contains(f.at(1, 1)) && !block.contains(f.at(1, 5)));
    assert!(!block.contains(f.at(3, 3)));
}

#[test]
fn contains_cell_wide_pair() {
    let mut f = fixture(5, 10);
    f.print("a中b");
    let screen = f.grid.screen();
    let row = screen.row(f.at(0, 0).row);
    let only_glyph = SelectionRange {
        start: f.at(0, 1),
        end: f.at(0, 1),
        is_block: false,
    };
    assert!(only_glyph.contains_cell(row, 1) && only_glyph.contains_cell(row, 2));
    assert!(!only_glyph.contains_cell(row, 0) && !only_glyph.contains_cell(row, 3));
    let only_spacer = SelectionRange {
        start: f.at(0, 2),
        end: f.at(0, 2),
        is_block: false,
    };
    assert!(only_spacer.contains_cell(row, 1) && only_spacer.contains_cell(row, 2));
}

// follow

#[test]
fn survives_scroll_into_history() {
    let mut f = fixture(3, 10);
    write_lines(&mut f, &["abc", "def", ""]);
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(1, 2));
    let before = f.range(&selection);
    assert_eq!(f.text(&selection).as_deref(), Some("abc\ndef"));
    for _ in 0..5 {
        f.newline();
    }
    assert_eq!(f.text(&selection).as_deref(), Some("abc\ndef"));
    assert_eq!(f.range(&selection), before);
    assert_eq!(f.range(&selection).map(|r| r.start), Some(f.at(-5, 0)));
    f.check();
}

#[test]
fn trimmed_end_resolves_none() {
    let mut f = fixture_with(3, 10, 2);
    write_lines(&mut f, &["abc", "def"]);
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(1, 2));
    assert!(f.range(&selection).is_some());
    for _ in 0..8 {
        f.newline();
    }
    assert_eq!(f.range(&selection), None);
    assert_eq!(f.text(&selection), None);
    f.check();
}

#[test]
fn viewport_scroll_changes_nothing() {
    let mut f = fixture(3, 10);
    write_lines(&mut f, &["abc", "def", "ghi", "jkl", "mno"]);
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(2, 2));
    let before = (f.range(&selection), f.text(&selection));
    f.grid.screen_mut().scroll_viewport(-2);
    assert_eq!((f.range(&selection), f.text(&selection)), before);
    f.check();
}

#[test]
fn region_scroll_moves_selection() {
    let mut f = fixture(6, 10);
    write_lines(&mut f, &["r0", "r1", "r2", "r3", "r4", "r5"]);
    let selection = f.span(SelectionKind::Simple, f.at(2, 0), f.at(3, 1));
    let region = ScrollRegion { top: 1, bottom: 5 };
    f.grid.scroll_up(region, 1);
    f.check();
    let selected = f.range(&selection).expect("a range");
    assert_eq!((selected.start, selected.end), (f.at(1, 0), f.at(2, 1)));
    assert_eq!(f.text(&selection).as_deref(), Some("r2\nr3"));
    f.grid.scroll_down(region, 2);
    f.check();
    assert_eq!(f.text(&selection).as_deref(), Some("r2\nr3"));
}

#[test]
fn region_scroll_stretches_selection() {
    let mut f = fixture(6, 10);
    write_lines(&mut f, &["r0", "r1", "r2", "r3", "r4", "r5"]);
    // The start is outside the region and stays; the end follows its row.
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(3, 1));
    f.grid.scroll_up(ScrollRegion { top: 1, bottom: 5 }, 1);
    f.check();
    let selected = f.range(&selection).expect("a range");
    assert_eq!((selected.start, selected.end), (f.at(0, 0), f.at(2, 1)));
}

#[test]
fn delete_lines_kills_end_on_deleted_row() {
    let mut f = fixture(6, 10);
    write_lines(&mut f, &["r0", "r1", "r2", "r3", "r4", "r5"]);
    let mut selection = f.span(SelectionKind::Simple, f.at(1, 0), f.at(3, 1));
    // Deleting a row between the ends shifts the end up with its content.
    f.goto(2, 0);
    f.grid.delete_lines(1);
    f.check();
    assert_eq!(f.text(&selection).as_deref(), Some("r1\nr3"));
    // Deleting the row of the end kills it.
    f.goto(2, 0);
    f.grid.delete_lines(1);
    f.check();
    assert_eq!(f.range(&selection), None);
    // Moving the end revives it, and the selection with it.
    let moved = f.at(2, 1);
    selection.update(&mut f.grid, moved, Right);
    assert_eq!(f.text(&selection).as_deref(), Some("r1\nr4"));
    f.check();
}

#[test]
fn survives_rows_only_resize() {
    let mut f = fixture(5, 10);
    write_lines(&mut f, &["abc", "def", "ghi"]);
    let selection = f.span(SelectionKind::Simple, f.at(0, 1), f.at(2, 1));
    for rows in [3, 8, 5] {
        f.resize(rows, 10, ResizePolicy::BottomAnchor);
        assert_eq!(f.text(&selection).as_deref(), Some("bc\ndef\ngh"));
    }
    f.resize(4, 10, ResizePolicy::KeepViewportTop);
    assert_eq!(f.text(&selection).as_deref(), Some("bc\ndef\ngh"));
}

#[test]
fn survives_column_reflow() {
    let mut f = fixture(5, 10);
    f.print("abcdefghijklmnopqrstuvwxyz");
    let selection = f.span(SelectionKind::Simple, f.at(0, 2), f.at(1, 4));
    assert_eq!(f.text(&selection).as_deref(), Some("cdefghijklmno"));
    for cols in [6, 10, 7] {
        f.resize(5, cols, ResizePolicy::BottomAnchor);
        assert_eq!(f.text(&selection).as_deref(), Some("cdefghijklmno"));
    }
}

#[test]
fn alt_clip_clamps_end_col() {
    let mut f = fixture(5, 10);
    f.grid.enter_alt();
    f.check();
    f.print("abcdefghij");
    let selection = f.span(SelectionKind::Simple, f.at(0, 2), f.at(0, 9));
    f.resize(5, 6, ResizePolicy::BottomAnchor);
    let selected = f.range(&selection).expect("a range");
    assert!(selected.end.col < 6);
    assert_eq!(f.text(&selection).as_deref(), Some("cdef"));
}

// invalidation

fn first_selection_on_rows_1_to_2(f: &mut Fixture) -> Selection {
    write_lines(f, &["aaa", "bbb", "ccc", "ddd", "eee"]);
    f.span(SelectionKind::Simple, f.at(1, 0), f.at(2, 2))
}

#[test]
fn invalidation_table() {
    use Invalidation::*;
    let mut f = fixture(5, 10);
    let selection = first_selection_on_rows_1_to_2(&mut f);
    let table = [
        (EraseLine, 3, false),
        (EraseLine, 2, true),
        (EraseLine, 1, true),
        (EraseLine, 0, false),
        (EraseBelow, 3, false),
        (EraseBelow, 1, true),
        (EraseBelow, 2, true),
        (EraseAbove, 0, false),
        (EraseAbove, 1, true),
        (EraseAbove, 3, true),
        (EraseScreen, 0, true),
        (EraseScreen, 4, true),
        (EraseHistory, 2, false),
        (SwapAlt, 4, true),
        (Reset, 0, true),
    ];
    for (inv, cursor_row, expected) in table {
        f.goto(cursor_row, 0);
        assert_eq!(
            selection.invalidated_by(&f.grid, inv),
            expected,
            "{inv:?} with the cursor on row {cursor_row}"
        );
    }
}

#[test]
fn history_selection_rules() {
    use Invalidation::*;
    let mut f = fixture(3, 10);
    write_lines(&mut f, &["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"]);
    let selection = f.span(SelectionKind::Simple, f.at(-4, 0), f.at(-3, 1));
    assert!(selection.invalidated_by(&f.grid, EraseHistory));
    assert!(selection.invalidated_by(&f.grid, EraseScreen));
    for inv in [EraseBelow, EraseAbove, EraseLine] {
        assert!(!selection.invalidated_by(&f.grid, inv), "{inv:?}");
    }
    f.check();
}

#[test]
fn inactive_screen_uses_its_own_cursor() {
    use Invalidation::*;
    let mut f = fixture(5, 10);
    let selection = first_selection_on_rows_1_to_2(&mut f);
    f.goto(2, 0);
    f.grid.enter_alt();
    f.goto(4, 0);
    f.check();
    assert!(f.grid.alt_active());
    // The primary cursor sits on row 2; the active (alternate) one on row 4 would say no.
    assert!(selection.invalidated_by(&f.grid, EraseLine));
    assert!(selection.invalidated_by(&f.grid, EraseBelow));
    assert!(selection.invalidated_by(&f.grid, SwapAlt));
}

#[test]
fn alt_screen_has_no_history_to_erase() {
    let mut f = fixture(5, 10);
    f.grid.enter_alt();
    f.print("abc");
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(0, 2));
    assert!(!selection.invalidated_by(&f.grid, Invalidation::EraseHistory));
    f.check();
}

#[test]
fn dead_end_invalidates_everything() {
    use Invalidation::*;
    let mut f = fixture_with(3, 10, 2);
    write_lines(&mut f, &["abc", "def"]);
    let selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(1, 2));
    for _ in 0..8 {
        f.newline();
    }
    assert_eq!(f.range(&selection), None);
    for inv in [
        EraseScreen,
        EraseAbove,
        EraseBelow,
        EraseLine,
        EraseHistory,
        Reset,
        SwapAlt,
    ] {
        assert!(selection.invalidated_by(&f.grid, inv), "{inv:?}");
    }
    f.check();
}

#[test]
fn dead_selection_end_revives_on_update() {
    let mut f = fixture_with(3, 10, 2);
    write_lines(&mut f, &["abc"]);
    let mut selection = f.span(SelectionKind::Simple, f.at(0, 0), f.at(0, 2));
    for _ in 0..8 {
        f.newline();
    }
    assert_eq!(f.grid.anchors().get(selection.end), None);
    let moved = f.at(2, 2);
    selection.update(&mut f.grid, moved, Right);
    assert!(f.grid.anchors().get(selection.end).is_some());
    f.check();
}
