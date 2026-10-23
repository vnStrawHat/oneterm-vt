use super::*;
use crate::cell::{Cell, CellContent, CellWidth, Color, Style};
use crate::grid::anchor::{AnchorId, AnchorKind};
use crate::grid::grid_fixture::{Fixture, fixture, fixture_with};
use crate::grid::screen::{CursorOrigin, DisplayClear};
use crate::grid::{Pos, RowId};
use crate::intern::Extras;

use ResizePolicy::{BottomAnchor, KeepViewportTop};

impl Fixture {
    /// Registers a mark on the active screen at a screen index and column.
    pub(crate) fn mark(&mut self, index: u16, col: u16) -> AnchorId {
        let row = self.grid.screen().row_of_index(index);
        self.grid
            .anchors
            .register(AnchorKind::Mark(1), Pos { row, col })
    }
}

/// The live rows of the active screen: text without trailing blanks, and the wrap flag.
fn rows(f: &Fixture) -> Vec<(String, bool)> {
    f.live_ids()
        .into_iter()
        .map(|id| {
            let text = f.row_text(id).trim_end().to_string();
            (text, f.grid.screen().row(id).wrapped())
        })
        .collect()
}

fn texts(f: &Fixture) -> Vec<String> {
    rows(f).into_iter().map(|(text, _)| text).collect()
}

fn mark_pos(f: &Fixture, id: AnchorId) -> Option<Pos> {
    f.grid.anchors.get(id)
}

/// The text of the row a position is on, and its column.
fn mark_text(f: &Fixture, id: AnchorId) -> Option<(String, u16)> {
    let pos = mark_pos(f, id)?;
    Some((f.row_text(pos.row).trim_end().to_string(), pos.col))
}

fn cursor_text(f: &Fixture) -> String {
    f.row_text(f.grid.screen().cursor().pos.row)
        .trim_end()
        .to_string()
}

fn lines_of_numbers(f: &mut Fixture, count: usize) {
    for i in 0..count {
        f.print(&format!("L{i}"));
        f.newline();
    }
}

// shrink and grow

#[test]
fn shrink_rewraps_long_line() {
    let mut f = fixture_with(3, 10, 10);
    f.print("abcdefghijkl");
    let lines = f.lines();
    let outcome = f.resize(3, 5, BottomAnchor);
    assert!(outcome.reflowed);
    assert_eq!(outcome.rows_trimmed, 0);
    assert_eq!(
        rows(&f)[..3],
        [
            ("abcde".to_string(), true),
            ("fghij".to_string(), true),
            ("kl".to_string(), false)
        ]
    );
    assert_eq!(f.lines(), lines);
}

#[test]
fn grow_joins_wrapped_rows() {
    let mut f = fixture_with(3, 10, 10);
    f.print("abcdefghijkl");
    let outcome = f.resize(3, 20, BottomAnchor);
    assert!(outcome.reflowed);
    assert_eq!(rows(&f)[0], ("abcdefghijkl".to_string(), false));
    assert_eq!(f.lines(), ["abcdefghijkl", "", ""]);
}

#[test]
fn line_ends_never_join() {
    let mut f = fixture(3, 10);
    f.print("ab");
    f.newline();
    f.print("cd");
    f.resize(3, 20, BottomAnchor);
    assert_eq!(
        rows(&f)[..2],
        [("ab".to_string(), false), ("cd".to_string(), false)]
    );
}

#[test]
fn empty_lines_stay_one_row_each() {
    let mut f = fixture(6, 10);
    f.print("a");
    for _ in 0..3 {
        f.newline();
    }
    f.print("b");
    let lines = f.lines();
    assert_eq!(lines[..4], ["a", "", "", "b"]);
    f.resize(6, 5, BottomAnchor);
    assert_eq!(f.lines(), lines);
    f.resize(6, 20, BottomAnchor);
    assert_eq!(f.lines(), lines);
    assert_eq!(texts(&f)[..4], ["a", "", "", "b"]);
}

#[test]
fn trailing_empty_trimmed_styled_blanks_kept() {
    let mut f = fixture(4, 10);
    f.print("ab");
    f.goto(2, 0);
    f.resize(4, 5, BottomAnchor);
    assert_eq!(rows(&f)[0], ("ab".to_string(), false));

    let mut f = fixture(4, 10);
    f.print("ab");
    f.set_style(Style {
        bg: Color::Palette(4),
        ..Style::DEFAULT
    });
    f.print("      ");
    f.goto(2, 0);
    f.resize(4, 5, BottomAnchor);
    assert_eq!(rows(&f)[0], ("ab".to_string(), true));
    assert!(!rows(&f)[1].1);
}

#[test]
fn grapheme_and_extras_ids_survive() {
    let mut f = fixture(3, 10);
    f.print("abcd");
    f.print("e\u{301}");
    f.print("fghijk");
    let extras = f.interner.extras(Extras {
        hyperlink: None,
        graphic: Some(crate::intern::GraphicId(1)),
    });
    let linked = Cell::EMPTY
        .with_content(CellContent::Scalar('z'))
        .with_extras(extras);
    let id = f.grid.screen().row_of_index(0);
    f.grid
        .screen_mut()
        .row_mut(id)
        .expect("live")
        .set(7, linked);
    f.check();
    let content = |f: &Fixture| -> Vec<Cell> {
        f.live_ids()
            .into_iter()
            .flat_map(|id| f.grid.screen().row(id).cells().to_vec())
            .filter(|&cell| cell != Cell::EMPTY)
            .collect()
    };
    let before = content(&f);
    assert!(
        before
            .iter()
            .any(|cell| matches!(cell.content(), CellContent::Grapheme(_)))
    );
    f.resize(3, 3, BottomAnchor);
    assert_eq!(content(&f), before);
    f.resize(3, 40, BottomAnchor);
    assert_eq!(content(&f), before);
}

// wide pairs

#[test]
fn wide_pair_never_split() {
    let mut f = fixture_with(1, 6, 10);
    f.print("abcd\u{FF21}");
    f.resize(1, 5, BottomAnchor);
    let screen = f.grid.primary();
    let first = screen.row(screen.oldest());
    assert_eq!(first.cell(4).width(), CellWidth::LeadingWideSpacer);
    assert!(first.wrapped());
    let second = screen.row(screen.newest());
    assert_eq!(second.cell(0).width(), CellWidth::Wide);
    assert_eq!(second.cell(1).width(), CellWidth::WideSpacer);
    assert_eq!(f.lines(), ["abcd\u{FF21}"]);

    f.resize(1, 6, BottomAnchor);
    let screen = f.grid.primary();
    let row = screen.row(screen.newest());
    assert_eq!(row.cell(4).width(), CellWidth::Wide);
    assert_eq!(row.cell(5).width(), CellWidth::WideSpacer);
    assert!(f.live_ids().into_iter().all(|id| {
        f.grid
            .screen()
            .row(id)
            .cells()
            .iter()
            .all(|cell| cell.width() != CellWidth::LeadingWideSpacer)
    }));
}

#[test]
fn leading_spacer_dropped_on_grow() {
    let mut f = fixture(2, 5);
    f.print("abcd\u{FF21}z");
    assert_eq!(
        f.grid
            .screen()
            .row(f.grid.screen().row_of_index(0))
            .cell(4)
            .width(),
        CellWidth::LeadingWideSpacer
    );
    f.resize(2, 20, BottomAnchor);
    let screen = f.grid.screen();
    let row = screen.row(screen.row_of_index(0));
    assert_eq!(row.cell(4).width(), CellWidth::Wide);
    assert_eq!(row.cell(5).width(), CellWidth::WideSpacer);
    assert_eq!(row.cell(6).content(), CellContent::Scalar('z'));
}

#[test]
fn one_column_degrades_wide() {
    let mut f = fixture(2, 6);
    f.print("a\u{FF21}b\u{FF22}");
    let before = f.grid.primary().dropped_wide();
    f.resize(2, 1, BottomAnchor);
    assert_eq!(f.grid.primary().dropped_wide(), before + 2);
    assert_eq!(f.lines()[0], "a  b");
    assert!(f.live_ids().into_iter().all(|id| {
        f.grid
            .screen()
            .row(id)
            .cells()
            .iter()
            .all(|cell| cell.width() == CellWidth::Narrow)
    }));
}

// cursor

#[test]
fn cursor_follows_its_char() {
    let mut f = fixture(3, 10);
    f.print("abcdefghijkl");
    f.resize(3, 5, BottomAnchor);
    assert_eq!(cursor_text(&f), "kl");
    assert_eq!(f.grid.screen().cursor().pos.col, 2);
    assert_eq!(f.cursor_at(), (0, 12, false));
    f.resize(3, 20, BottomAnchor);
    assert_eq!(cursor_text(&f), "abcdefghijkl");
    assert_eq!(f.grid.screen().cursor().pos.col, 12);
}

#[test]
fn pending_wrap_on_exact_fit() {
    let mut f = fixture(3, 10);
    f.print("abcdefghij");
    assert!(f.grid.screen().cursor().pending_wrap);
    f.resize(3, 5, BottomAnchor);
    let cursor = f.grid.screen().cursor();
    assert_eq!((cursor.pos.col, cursor.pending_wrap), (4, true));
    assert_eq!(cursor_text(&f), "fghij");
    f.resize(3, 20, BottomAnchor);
    let cursor = f.grid.screen().cursor();
    assert_eq!((cursor.pos.col, cursor.pending_wrap), (10, false));
}

#[test]
fn cursor_keeps_distance_from_text() {
    let mut f = fixture(3, 10);
    f.print("ab");
    f.goto(0, 7);
    f.resize(3, 4, BottomAnchor);
    let screen = f.grid.screen();
    let first = screen.oldest();
    assert!(screen.row(first).wrapped());
    assert_eq!(
        screen.cursor().pos,
        Pos {
            row: first + 1,
            col: 3
        }
    );
    assert_eq!(f.cursor_at(), (0, 7, false));
}

#[test]
fn saved_cursor_maps_like_cursor() {
    let mut f = fixture(3, 10);
    f.print("abcdefghijkl");
    f.grid.screen_mut().save_cursor();
    f.goto(2, 0);
    f.resize(3, 5, BottomAnchor);
    let saved = *f.grid.screen().saved_cursor();
    assert_eq!(f.row_text(saved.pos.row).trim_end(), "kl");
    assert_eq!(saved.pos.col, 2);
    assert!(!saved.pending_wrap);

    // An armed saved cursor stays armed on an exact fit.
    let mut f = fixture(3, 10);
    f.print("abcdefghij");
    f.grid.screen_mut().save_cursor();
    f.resize(3, 5, BottomAnchor);
    let saved = *f.grid.screen().saved_cursor();
    assert_eq!((saved.pos.col, saved.pending_wrap), (4, true));
}

#[test]
fn cursor_line_trimmed_goes_to_newest() {
    let mut f = fixture_with(2, 10, 0);
    f.goto(1, 0);
    f.print("abcdefghij");
    f.goto(0, 0);
    let outcome = f.resize(2, 1, BottomAnchor);
    assert!(outcome.rows_trimmed > 0);
    let screen = f.grid.screen();
    assert_eq!(
        screen.cursor().pos,
        Pos {
            row: screen.newest(),
            col: 0
        }
    );
    assert!(!screen.cursor().pending_wrap);
}

// anchors

#[test]
fn mark_follows_its_char() {
    let mut f = fixture(3, 10);
    f.print("abcdefghijkl");
    let l = f.mark(1, 1);
    let g = f.mark(0, 6);
    f.resize(3, 5, BottomAnchor);
    assert_eq!(mark_text(&f, l), Some(("kl".to_string(), 1)));
    assert_eq!(mark_text(&f, g), Some(("fghij".to_string(), 1)));
    f.resize(3, 20, BottomAnchor);
    assert_eq!(mark_text(&f, l), Some(("abcdefghijkl".to_string(), 11)));
}

#[test]
fn mark_on_trimmed_blanks_lands_at_line_end() {
    let mut f = fixture(3, 10);
    f.print("ab");
    let far = f.mark(0, 8);
    f.resize(3, 5, BottomAnchor);
    assert_eq!(mark_text(&f, far), Some(("ab".to_string(), 2)));
}

#[test]
fn selection_anchors_map_like_marks() {
    let mut f = fixture(3, 10);
    f.print("abcdefghijkl");
    let (r0, r1) = (
        f.grid.screen().row_of_index(0),
        f.grid.screen().row_of_index(1),
    );
    let start = f
        .grid
        .anchors
        .register(AnchorKind::SelectionStart, Pos { row: r0, col: 3 });
    let end = f
        .grid
        .anchors
        .register(AnchorKind::SelectionEnd, Pos { row: r1, col: 1 });
    f.resize(3, 5, BottomAnchor);
    assert_eq!(mark_text(&f, start), Some(("abcde".to_string(), 3)));
    assert_eq!(mark_text(&f, end), Some(("kl".to_string(), 1)));
}

#[test]
fn mark_in_trimmed_history_dies() {
    let mut f = fixture_with(2, 10, 3);
    for i in 0..5 {
        f.print(&i.to_string().repeat(9));
        f.newline();
    }
    let oldest = f.grid.screen().oldest();
    let old = f.grid.anchors.register(
        AnchorKind::Mark(9),
        Pos {
            row: oldest,
            col: 0,
        },
    );
    let outcome = f.resize(2, 5, BottomAnchor);
    assert!(outcome.rows_trimmed > 0);
    assert_eq!(f.grid.anchors.get(old), None);
    assert_eq!(f.grid.anchors.kind(old), Some(AnchorKind::Mark(9)));
}

#[test]
fn screen_owned_never_die() {
    let mut f = fixture_with(2, 10, 0);
    f.goto(1, 0);
    f.print("abcdefghij");
    f.goto(0, 0);
    for policy in [BottomAnchor, KeepViewportTop] {
        f.resize(2, 1, policy);
        f.resize(3, 7, policy);
        f.resize(2, 10, policy);
    }
    for (id, pos) in f.grid.primary().owned_anchors() {
        assert_eq!(f.grid.anchors.get(id), Some(pos));
    }
}

// rows

#[test]
fn grow_pulls_history_first() {
    let mut f = fixture(4, 10);
    lines_of_numbers(&mut f, 10);
    let screen = f.grid.screen();
    let (history, index, oldest, newest) = (
        screen.history_len(),
        screen.cursor_row_index(),
        screen.oldest(),
        screen.newest(),
    );
    let cursor_row = screen.cursor().pos.row;
    let outcome = f.resize(6, 10, BottomAnchor);
    assert!(!outcome.reflowed);
    let screen = f.grid.screen();
    assert_eq!(screen.history_len(), history - 2);
    assert_eq!(screen.cursor_row_index(), index + 2);
    assert_eq!((screen.oldest(), screen.newest()), (oldest, newest));
    assert_eq!(screen.cursor().pos.row, cursor_row);
}

#[test]
fn grow_without_history_appends_blank_rows() {
    let mut f = fixture(4, 10);
    f.print("x");
    let (oldest, newest) = (f.grid.screen().oldest(), f.grid.screen().newest());
    f.resize(6, 10, BottomAnchor);
    let screen = f.grid.screen();
    assert_eq!(screen.oldest(), oldest);
    assert_eq!(screen.newest(), newest + 2);
    assert!(!screen.row(newest + 1).is_allocated());
    assert!(!screen.row(newest + 2).is_allocated());
    assert_eq!(screen.history_len(), 0);
}

#[test]
fn shrink_cursor_low_pushes_into_history() {
    let mut f = fixture(5, 10);
    f.label_rows();
    let newest = f.grid.screen().newest();
    let outcome = f.resize(3, 10, BottomAnchor);
    assert_eq!(outcome.rows_trimmed, 0);
    let screen = f.grid.screen();
    assert_eq!(screen.history_len(), 2);
    assert_eq!(screen.newest(), newest);
    assert_eq!(f.screen_text(), "r2|r3|r4");
    assert_eq!(f.grid.screen().cursor_row_index(), 2);
}

#[test]
fn shrink_cursor_high_drops_blank_rows() {
    let mut f = fixture(5, 10);
    f.print("x");
    let newest = f.grid.screen().newest();
    f.resize(3, 10, BottomAnchor);
    let screen = f.grid.screen();
    assert_eq!(screen.history_len(), 0);
    assert_eq!(screen.newest(), newest - 2);
    assert_eq!(f.screen_text(), "x||");
}

#[test]
fn shrink_keeps_content_below_cursor() {
    let mut f = fixture(5, 10);
    f.goto(4, 0);
    f.print("tail");
    f.goto(2, 0);
    f.resize(3, 10, BottomAnchor);
    assert_eq!(f.grid.screen().history_len(), 2);
    assert_eq!(f.grid.screen().cursor_row_index(), 0);
    assert_eq!(f.screen_text(), "||tail");
}

#[test]
fn released_id_comes_back_stamped() {
    let mut f = fixture(5, 10);
    f.print("x");
    let newest = f.grid.screen().newest();
    f.resize(3, 10, BottomAnchor);
    let released = newest - 1;
    assert!(f.grid.screen().newest() < released);
    let batch = f.grid.begin_batch();
    f.goto(2, 0);
    f.grid.linefeed();
    f.check();
    let row = f.grid.screen().row(released);
    assert!(row.is_allocated());
    assert_eq!(row.seq(), batch);
    // A never-issued id above it stays unwritten.
    f.grid.linefeed();
    assert!(!f.grid.screen().row(newest + 1).is_allocated());
}

#[test]
fn region_reset_on_resize() {
    let mut f = fixture(6, 10);
    f.grid.screen_mut().set_region(1, 4, CursorOrigin::Screen);
    f.resize(6, 20, BottomAnchor);
    assert_eq!(f.grid.primary().region(), ScrollRegion::full(6));
    f.grid.screen_mut().set_region(1, 4, CursorOrigin::Screen);
    f.resize(4, 20, BottomAnchor);
    assert_eq!(f.grid.primary().region(), ScrollRegion::full(4));
    assert_eq!(f.grid.alt().region(), ScrollRegion::full(4));
}

#[test]
fn tab_stops_resize() {
    let mut f = fixture(3, 20);
    f.grid.screen_mut().tabs_mut().clear(8);
    f.resize(3, 10, BottomAnchor);
    assert!(!f.grid.screen_mut().tabs_mut().is_stop(8));
    f.resize(3, 30, BottomAnchor);
    let tabs = f.grid.screen_mut().tabs_mut();
    assert!(!tabs.is_stop(8));
    assert!(tabs.is_stop(16) && tabs.is_stop(24) && tabs.is_stop(0));
    assert!(!tabs.is_stop(30));
}

// bounds

#[test]
fn size_is_clamped() {
    let mut f = fixture(3, 10);
    f.resize(0, 0, BottomAnchor);
    assert_eq!(f.grid.primary().size(), Size { rows: 1, cols: 1 });
    assert_eq!(f.grid.alt().size(), Size { rows: 1, cols: 1 });
    f.resize(u16::MAX, u16::MAX, BottomAnchor);
    assert_eq!(
        f.grid.primary().size(),
        Size {
            rows: 1024,
            cols: 2048
        }
    );
    assert_eq!(
        f.grid.alt().size(),
        Size {
            rows: 1024,
            cols: 2048
        }
    );
}

#[test]
fn same_size_is_a_noop() {
    let mut f = fixture(3, 10);
    f.print("hello");
    let seq = f.grid.seq;
    let outcome = f.resize(3, 10, KeepViewportTop);
    assert_eq!(outcome, ResizeOutcome::default());
    assert_eq!(f.grid.seq, seq);
    // The clamped size counts: an oversized request that clamps to the current size is a no-op.
    let mut big = fixture(1024, 2048);
    let seq = big.grid.seq;
    assert_eq!(
        big.resize(u16::MAX, u16::MAX, BottomAnchor),
        ResizeOutcome::default()
    );
    assert_eq!(big.grid.seq, seq);
}

// alternate screen

#[test]
fn alt_clips_never_reflows() {
    let mut f = fixture(3, 10);
    f.print("abcdefghijkl");
    f.grid.enter_alt();
    f.check();
    f.goto(0, 0);
    f.print("ABCDEFGHIJ");
    let (oldest, newest) = (f.grid.alt().oldest(), f.grid.alt().newest());
    let outcome = f.resize(3, 5, BottomAnchor);
    assert!(outcome.reflowed);
    let alt = f.grid.alt();
    assert_eq!((alt.oldest(), alt.newest()), (oldest, newest));
    assert_eq!(f.index_text(0).trim_end(), "ABCDE");
    assert!(!alt.row(oldest).wrapped());
    let primary = f.grid.primary();
    let mut text = String::new();
    primary.row_text(primary.oldest(), &f.interner.graphemes, &mut text);
    assert_eq!(text, "abcde");
    f.resize(3, 12, BottomAnchor);
    assert_eq!(f.index_text(0).trim_end(), "ABCDE");
}

#[test]
fn alt_pending_wrap_rules() {
    let mut f = fixture(3, 10);
    f.grid.enter_alt();
    f.goto(0, 0);
    f.print("ABCDEFGHIJ");
    assert!(f.grid.alt().cursor().pending_wrap);
    f.resize(3, 5, BottomAnchor);
    let cursor = f.grid.alt().cursor();
    assert_eq!((cursor.pos.col, cursor.pending_wrap), (4, true));
    f.resize(3, 20, BottomAnchor);
    let cursor = f.grid.alt().cursor();
    assert_eq!((cursor.pos.col, cursor.pending_wrap), (4, false));

    // A cursor that was not armed stays unarmed when clamped.
    f.goto(0, 15);
    f.resize(3, 10, BottomAnchor);
    let cursor = f.grid.alt().cursor();
    assert_eq!((cursor.pos.col, cursor.pending_wrap), (9, false));
}

#[test]
fn alt_anchors_clamp_columns() {
    let mut f = fixture(3, 10);
    f.grid.enter_alt();
    let far = f.mark(0, 8);
    f.resize(3, 4, BottomAnchor);
    assert_eq!(mark_pos(&f, far).map(|pos| pos.col), Some(3));
}

#[test]
fn primary_reflows_while_alt_active() {
    let mut f = fixture(3, 10);
    f.print("abcdefghijkl");
    f.grid.enter_alt();
    f.check();
    let outcome = f.resize(3, 5, BottomAnchor);
    assert!(outcome.reflowed);
    f.grid.leave_alt();
    f.check();
    assert_eq!(f.lines(), ["abcdefghijkl", ""]);
    assert_eq!(texts(&f)[..3], ["abcde", "fghij", "kl"]);
}

// scrollback

#[test]
fn zero_scrollback_reflow() {
    let mut f = fixture_with(3, 10, 0);
    f.print("abcdefghijkl");
    let outcome = f.resize(3, 2, BottomAnchor);
    assert_eq!(outcome.rows_trimmed, 3);
    assert_eq!(f.grid.screen().history_len(), 0);
    assert_eq!(texts(&f), ["gh", "ij", "kl"]);
}

#[test]
fn max_scrollback_trim() {
    let mut f = fixture_with(3, 10, 20);
    for i in 0..30 {
        f.print(&format!("{i:09}"));
        f.newline();
    }
    assert_eq!(f.grid.screen().history_len(), 20);
    let lines = f.lines().len();
    let output = 2 * (lines - 1) + 1;
    let outcome = f.resize(3, 5, BottomAnchor);
    assert_eq!(f.grid.screen().history_len(), 20);
    assert_eq!(outcome.rows_trimmed as usize, output - 20 - 3);
}

// policy

#[test]
fn bottom_anchor_grow_pulls_history() {
    let mut f = fixture(3, 10);
    lines_of_numbers(&mut f, 6);
    let before = f.grid.screen().history_len();
    f.resize(5, 10, BottomAnchor);
    assert_eq!(f.grid.screen().history_len(), before - 2);
    assert_eq!(f.grid.screen().cursor_row_index(), 2 + 2);
}

#[test]
fn keep_viewport_top_grow_appends_blank() {
    let mut f = fixture(3, 10);
    lines_of_numbers(&mut f, 6);
    let top = f.grid.screen().screen_top();
    let index = f.grid.screen().cursor_row_index();
    let history = f.grid.screen().history_len();
    f.resize(5, 10, KeepViewportTop);
    let screen = f.grid.screen();
    assert_eq!(screen.screen_top(), top);
    assert_eq!(screen.cursor_row_index(), index);
    assert_eq!(screen.history_len(), history);
    assert_eq!(f.index_text(3).trim_end(), "");
}

#[test]
fn keep_viewport_top_matches_host_row() {
    let mut f = fixture_with(2, 10, 5);
    f.print("abcdefghijklmnopqrstuvwxy");
    let host = f.grid.primary().host_cursor_row(Size { rows: 2, cols: 20 });
    assert_eq!(host, 0);
    f.resize(2, 20, KeepViewportTop);
    let screen = f.grid.screen();
    assert_eq!(screen.cursor_row_index(), host);
    assert_eq!(f.index_text(0).trim_end(), "uvwxy");
    assert!(screen.row(screen.screen_top() - 1).wrapped());
    assert_eq!(screen.history_len(), 1);

    // And the same measure on a shrink: the cursor goes where the host wrapped it.
    let mut f = fixture_with(4, 10, 5);
    f.print("0123456789ab");
    let host = f.grid.primary().host_cursor_row(Size { rows: 4, cols: 5 });
    assert_eq!(host, 2);
    f.resize(4, 5, KeepViewportTop);
    assert_eq!(f.grid.screen().cursor_row_index(), host);
    assert_eq!(cursor_text(&f), "ab");
}

#[test]
fn view_at_bottom_stays_at_bottom() {
    for policy in [BottomAnchor, KeepViewportTop] {
        let mut f = fixture(3, 10);
        lines_of_numbers(&mut f, 8);
        for (rows, cols) in [(3, 5), (5, 5), (2, 20), (3, 10)] {
            f.resize(rows, cols, policy);
            assert_eq!(f.grid.screen().scroll_offset(), 0);
        }
    }
}

#[test]
fn scrolled_back_view_keeps_first_char() {
    for policy in [BottomAnchor, KeepViewportTop] {
        let mut f = fixture(3, 10);
        lines_of_numbers(&mut f, 12);
        f.grid.screen_mut().scroll_viewport(-4);
        let top = |f: &Fixture| {
            f.row_text(f.grid.screen().visible_top())
                .trim_end()
                .to_string()
        };
        let first = top(&f);
        assert_eq!(f.grid.screen().scroll_offset(), 4);
        for (rows, cols) in [(3, 20), (4, 5), (3, 10), (6, 10), (2, 10)] {
            f.resize(rows, cols, policy);
            assert_eq!(top(&f), first, "{policy:?} {rows}x{cols}");
        }
    }
}

#[test]
fn scrolled_back_view_clamps_to_history() {
    let mut f = fixture_with(3, 10, 4);
    lines_of_numbers(&mut f, 12);
    f.grid.screen_mut().scroll_viewport(-4);
    f.resize(3, 5, BottomAnchor);
    assert!(f.grid.screen().scroll_offset() <= f.grid.screen().history_len());
    f.grid.screen_mut().scroll_to_bottom();
    f.grid.screen_mut().scroll_viewport(-4);
    f.resize(3, 3, KeepViewportTop);
    assert!(f.grid.screen().scroll_offset() <= f.grid.screen().history_len());
}

// outcome

#[test]
fn outcome_rows_only_not_reflowed() {
    let mut f = fixture(5, 10);
    f.label_rows();
    let outcome = f.resize(3, 10, BottomAnchor);
    assert_eq!(outcome, ResizeOutcome::default());
    let outcome = f.resize(3, 12, BottomAnchor);
    assert!(outcome.reflowed);
}

#[test]
fn outcome_counts_row_shrink_trim() {
    let mut f = fixture_with(5, 10, 1);
    f.label_rows();
    let outcome = f.resize(2, 10, BottomAnchor);
    assert_eq!(outcome.rows_trimmed, 2);
    assert!(!outcome.reflowed);
}

#[test]
fn outcome_counts_shift_trim() {
    // A shift that opens more rows than the scrollback holds trims the oldest, and the
    // trimmed rows are reported to the caller of `shift_cursor_to`.
    let mut f = fixture_with(3, 10, 1);
    f.label_rows();
    let top = f.mark(0, 0);
    let trimmed = f.grid.primary.shift_cursor_to(0, &mut f.grid.anchors);
    f.check();
    assert_eq!(trimmed, 1);
    assert_eq!(f.grid.anchors.get(top), None);
    assert_eq!(f.grid.screen().cursor_row_index(), 0);
}

#[test]
fn erase_after_resize_keeps_working() {
    let mut f = fixture(4, 10);
    f.print("abcdefghijkl");
    f.resize(4, 5, BottomAnchor);
    f.grid.erase_display(DisplayClear::All, &f.interner);
    f.check();
    f.print("zz");
    assert_eq!(
        rows(&f)
            .iter()
            .filter(|(text, _)| text.trim_start() == "zz")
            .count(),
        1
    );
}

#[test]
fn ids_are_fresh_on_column_change_and_kept_on_row_change() {
    let mut f = fixture(3, 10);
    lines_of_numbers(&mut f, 5);
    let before = f.grid.screen().newest();
    f.resize(3, 12, BottomAnchor);
    assert!(f.grid.screen().oldest() > before);
    let (oldest, newest) = (f.grid.screen().oldest(), f.grid.screen().newest());
    f.resize(5, 12, BottomAnchor);
    assert_eq!(f.grid.screen().oldest(), oldest);
    assert!(f.grid.screen().newest() >= newest);
    let _: RowId = f.grid.primary().newest();
}

#[test]
fn keep_viewport_top_pulls_history_to_follow_the_host() {
    let build = |policy| {
        let mut f = fixture_with(4, 10, 20);
        f.print("aaaaaaaaa");
        f.newline();
        f.print("bbbbbbbbb");
        f.resize(4, 5, policy);
        f
    };
    // The host keeps the top row: both lines wrap in place and the cursor goes to row 3.
    let f = build(KeepViewportTop);
    assert_eq!(f.screen_text(), "aaaaa|aaaa|bbbbb|bbbb");
    assert_eq!(f.grid.screen().cursor_row_index(), 3);
    assert_eq!(f.grid.screen().history_len(), 0);
    // The bottom stays at the bottom: the first line scrolls into history.
    let f = build(BottomAnchor);
    assert_eq!(f.screen_text(), "bbbbb|bbbb||");
    assert_eq!(f.grid.screen().cursor_row_index(), 1);
    assert_eq!(f.grid.screen().history_len(), 2);
}

#[test]
fn repeated_resize_allocates_nothing() {
    let mut f = fixture(4, 10);
    lines_of_numbers(&mut f, 12);
    f.resize(5, 20, BottomAnchor);
    let bytes = f.grid.heap_bytes();
    assert_eq!(f.resize(5, 20, BottomAnchor), ResizeOutcome::default());
    assert_eq!(f.grid.heap_bytes(), bytes);
}

#[test]
fn resize_cycles_reach_a_steady_heap() {
    // Growing pulls history in and shrinking pushes it back, so primary rows only change
    // places. The alternate screen releases its blank rows on a shrink and re-issues them
    // stamped on the next grow, which costs buffers for the first cycles only.
    let mut f = fixture_with(4, 10, 20);
    lines_of_numbers(&mut f, 30);
    for _ in 0..2 {
        f.resize(8, 10, BottomAnchor);
        f.resize(4, 10, BottomAnchor);
    }
    let bytes = f.grid.heap_bytes();
    for _ in 0..5 {
        f.resize(8, 10, BottomAnchor);
        f.resize(4, 10, BottomAnchor);
        assert_eq!(f.grid.heap_bytes(), bytes);
    }
}

#[test]
fn reflow_scratch_is_empty_between_calls_and_counted() {
    let mut f = fixture(4, 10);
    lines_of_numbers(&mut f, 12);
    f.resize(4, 6, BottomAnchor);
    assert!(f.grid.reflow_rows.is_empty());
    let scratch = f.grid.reflow_rows.capacity() * size_of::<Option<crate::grid::Row>>();
    assert!(scratch > 0);
    let others =
        f.grid.primary().heap_bytes() + f.grid.alt().heap_bytes() + f.grid.anchors.heap_bytes();
    assert_eq!(f.grid.heap_bytes(), others + scratch);
}

#[test]
fn shrink_back_does_not_evict_content_for_padding() {
    let mut f = fixture_with(5, 10, 0);
    f.print("abcdefghijkl");
    f.resize(5, 20, BottomAnchor);
    let outcome = f.resize(5, 10, BottomAnchor);
    f.check();
    assert_eq!(texts(&f), ["abcdefghij", "kl", "", "", ""]);
    assert_eq!(outcome.rows_trimmed, 0);
}

#[test]
fn shrink_back_keeps_history_at_default_scrollback() {
    let mut f = fixture_with(5, 10, 10_000);
    for i in 0..100 {
        f.print(&i.to_string());
        f.newline();
    }
    assert_eq!(f.grid.screen().history_len(), 96);
    let before = f.lines();
    f.resize(5, 20, BottomAnchor);
    let outcome = f.resize(5, 10, BottomAnchor);
    f.check();
    assert_eq!(outcome.rows_trimmed, 0);
    assert_eq!(f.grid.screen().history_len(), 96);
    assert_eq!(f.lines(), before);
}
