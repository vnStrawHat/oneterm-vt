use std::collections::BTreeSet;

use super::*;
use crate::cell::{Attrs, CellWidth, Color, Rgb};
use crate::grid::grid_fixture::{Fixture, fixture, fixture_with};
use crate::grid::row::RowFlags;
use crate::intern::{Extras, GraphicId};

const WIDE_A: &str = "\u{FF21}";
const WIDE_B: &str = "\u{FF22}";

fn screen(f: &Fixture) -> &Screen {
    f.grid.screen()
}

fn scroll_up_full(f: &mut Fixture, n: u16) -> ScrollReport {
    let region = ScrollRegion::full(screen(f).rows());
    scroll_up(f, region, n)
}

fn scroll_down_full(f: &mut Fixture, n: u16) -> ScrollReport {
    let region = ScrollRegion::full(screen(f).rows());
    scroll_down(f, region, n)
}

fn newlines(f: &mut Fixture, count: usize) {
    for _ in 0..count {
        f.newline();
    }
}

fn scroll_up(f: &mut Fixture, region: ScrollRegion, n: u16) -> ScrollReport {
    let report = f.grid.scroll_up(region, n);
    f.check();
    report
}

fn scroll_down(f: &mut Fixture, region: ScrollRegion, n: u16) -> ScrollReport {
    let report = f.grid.scroll_down(region, n);
    f.check();
    report
}

fn erase_display(f: &mut Fixture, mode: DisplayClear) -> Option<ScrollReport> {
    let report = f.grid.erase_display(mode, &f.interner);
    f.check();
    report
}

fn history_text(f: &Fixture, back: u64) -> String {
    f.row_text(screen(f).screen_top() - back)
}

fn bg_pen() -> Style {
    Style {
        fg: Color::Rgb(Rgb { r: 255, g: 0, b: 0 }),
        bg: Color::Palette(4),
        attrs: Attrs::BOLD | Attrs::UNDERLINE,
        ..Style::DEFAULT
    }
}

// ids and viewport

#[test]
fn offset_zero_follows_output() {
    let mut f = fixture(4, 10);
    newlines(&mut f, 10);
    assert_eq!(screen(&f).scroll_offset(), 0);
    assert_eq!(screen(&f).history_len(), 7);
}

#[test]
fn scrolled_back_view_holds_still() {
    let mut f = fixture(4, 10);
    newlines(&mut f, 10);
    f.grid.screen_mut().scroll_viewport(-3);
    let top = screen(&f).visible_top();
    newlines(&mut f, 2);
    assert_eq!(screen(&f).scroll_offset(), 5);
    assert_eq!(screen(&f).visible_top(), top);
}

#[test]
fn offset_capped_by_history() {
    let mut f = fixture(4, 10);
    newlines(&mut f, 10);
    f.grid.screen_mut().scroll_viewport(-10_000);
    assert_eq!(screen(&f).scroll_offset(), screen(&f).history_len());
    assert_eq!(screen(&f).visible_top(), screen(&f).oldest());
    f.grid.screen_mut().scroll_viewport(10_000);
    assert_eq!(screen(&f).scroll_offset(), 0);
    f.grid.screen_mut().scroll_viewport(-2);
    f.grid.screen_mut().scroll_to_bottom();
    assert_eq!(screen(&f).scroll_offset(), 0);
    f.check();
}

#[test]
fn viewport_reports_the_visible_rows() {
    let mut f = fixture(4, 10);
    newlines(&mut f, 6);
    f.grid.screen_mut().scroll_viewport(-2);
    let viewport = screen(&f).viewport();
    assert_eq!(viewport.top, screen(&f).visible_top());
    assert_eq!((viewport.rows, viewport.cols), (4, 10));
}

#[test]
fn ids_never_rewind() {
    let mut f = fixture_with(3, 10, 4);
    let mut last = screen(&f).newest();
    let mut seen = |f: &Fixture| {
        assert!(f.grid.screen().newest() >= last);
        last = f.grid.screen().newest();
    };
    newlines(&mut f, 8);
    seen(&f);
    erase_display(&mut f, DisplayClear::All);
    seen(&f);
    erase_display(&mut f, DisplayClear::Saved);
    seen(&f);
    f.grid.reset();
    f.check();
    seen(&f);
    assert!(screen(&f).newest() >= RowId(8));
}

#[test]
fn ring_len_rounds_up() {
    let mut anchors = Anchors::new();
    let size = Size { rows: 4, cols: 10 };
    for (limit, want) in [(10_000, 16_384), (0, 1024), (u32::MAX, 1_048_576)] {
        let screen = Screen::new(ScreenKind::Primary, size, limit, &mut anchors);
        assert_eq!(screen.ring_len(), want);
        assert_eq!(screen.ring_mask(), want as u64 - 1);
    }
}

#[test]
fn scrollback_limit_clamps_to_the_maximum() {
    let mut anchors = Anchors::new();
    let size = Size { rows: 4, cols: 10 };
    let screen = Screen::new(ScreenKind::Primary, size, u32::MAX, &mut anchors);
    assert_eq!(screen.scrollback_limit, SCROLLBACK_MAX);
}

#[test]
fn unwritten_slots_read_as_blanks() {
    let f = fixture(4, 10);
    for id in [RowId(0), RowId(3), RowId(77), RowId::ALT_ORIGIN] {
        let row = screen(&f).row(id);
        assert!(!row.is_allocated());
        assert_eq!(row.cells(), &[Cell::EMPTY; 10]);
    }
    assert_eq!(f.index_text(0), " ".repeat(10));
}

#[test]
fn row_mut_refuses_ids_outside_the_live_range() {
    let mut f = fixture_with(4, 10, 3);
    newlines(&mut f, 20);
    let (oldest, newest) = (screen(&f).oldest(), screen(&f).newest());
    assert!(f.grid.screen_mut().row_mut(oldest - 1).is_none());
    assert!(f.grid.screen_mut().row_mut(newest + 1).is_none());
    assert!(f.grid.screen_mut().row_mut(oldest).is_some());
}

#[test]
fn trimmed_slot_cleared_before_reuse() {
    let mut f = fixture_with(2, 10, 2);
    let ring = screen(&f).ring_len();
    for _ in 0..ring + 5 {
        f.print("x");
        f.newline();
    }
    let range = screen(&f).row_range();
    assert_eq!(range.end - range.start.0, RowId(4));
    let mut id = range.start;
    while id < range.end {
        let row = screen(&f).row(id);
        assert_eq!(row.id(), id);
        let want = if id == screen(&f).newest() { " " } else { "x" };
        assert_eq!(&f.row_text(id)[..1], want);
        id = id + 1;
    }
}

#[test]
fn history_stays_within_limit() {
    let mut f = fixture_with(3, 10, 3);
    newlines(&mut f, 50);
    assert_eq!(screen(&f).history_len(), 3);
    assert_eq!(screen(&f).row_range().end - 6, screen(&f).oldest() + 0);
}

// cursor and region

#[test]
fn set_region_validates_and_homes() {
    let mut f = fixture(6, 10);
    f.goto(3, 3);
    assert!(f.grid.screen_mut().set_region(1, 4, CursorOrigin::Screen));
    assert_eq!(screen(&f).region(), ScrollRegion { top: 1, bottom: 4 });
    assert_eq!(screen(&f).cursor_row_index(), 0);
    f.goto(3, 3);
    assert!(!f.grid.screen_mut().set_region(4, 4, CursorOrigin::Screen));
    assert_eq!(screen(&f).region(), ScrollRegion { top: 1, bottom: 4 });
    assert_eq!(
        (screen(&f).cursor_row_index(), screen(&f).cursor().pos.col),
        (3, 3)
    );
    assert!(f.grid.screen_mut().set_region(2, 5, CursorOrigin::Region));
    assert_eq!(screen(&f).cursor_row_index(), 2);
    assert!(f.grid.screen_mut().set_region(0, 99, CursorOrigin::Screen));
    assert_eq!(screen(&f).region(), ScrollRegion::full(6));
    f.check();
}

#[test]
fn goto_origin_clamps_to_region() {
    let mut f = fixture(6, 10);
    f.grid.screen_mut().set_region(1, 4, CursorOrigin::Screen);
    f.grid
        .screen_mut()
        .goto_origin(10, 99, CursorOrigin::Region);
    assert_eq!(screen(&f).cursor_row_index(), 3);
    assert_eq!(screen(&f).cursor().pos.col, 9);
    f.grid.screen_mut().goto_origin(10, 0, CursorOrigin::Screen);
    assert_eq!(screen(&f).cursor_row_index(), 5);
    f.check();
}

#[test]
fn linefeed_below_region_stops_at_bottom() {
    let mut f = fixture(5, 10);
    f.grid.screen_mut().set_region(0, 3, CursorOrigin::Screen);
    f.label_rows();
    f.goto(3, 0);
    f.grid.linefeed();
    f.check();
    assert_eq!(screen(&f).cursor_row_index(), 4);
    f.grid.linefeed();
    f.check();
    assert_eq!(screen(&f).cursor_row_index(), 4);
    assert_eq!(screen(&f).history_len(), 0);
}

#[test]
fn linefeed_at_region_bottom_scrolls_the_region() {
    let mut f = fixture(5, 10);
    f.grid.screen_mut().set_region(1, 4, CursorOrigin::Screen);
    f.label_rows();
    f.goto(3, 0);
    f.grid.linefeed();
    f.check();
    assert_eq!(f.screen_text(), "r0|r2|r3||r4");
    assert_eq!(screen(&f).cursor_row_index(), 3);
}

#[test]
fn reverse_index_scrolls_down_at_region_top_and_moves_up_elsewhere() {
    let mut f = fixture(4, 10);
    f.label_rows();
    f.goto(2, 0);
    f.grid.reverse_index();
    f.check();
    assert_eq!(screen(&f).cursor_row_index(), 1);
    f.goto(0, 0);
    f.grid.reverse_index();
    f.check();
    assert_eq!(f.screen_text(), "|r0|r1|r2");
    f.grid.screen_mut().set_region(1, 3, CursorOrigin::Screen);
    f.goto(0, 0);
    f.grid.reverse_index();
    f.check();
    assert_eq!(screen(&f).cursor_row_index(), 0);
}

#[test]
fn il_dl_outside_region_are_noops() {
    let mut f = fixture(5, 10);
    f.label_rows();
    f.grid.screen_mut().set_region(1, 3, CursorOrigin::Screen);
    f.goto(0, 0);
    let before = f.screen_text();
    assert_eq!(f.grid.insert_lines(1), None);
    assert_eq!(f.grid.delete_lines(1), None);
    f.goto(4, 0);
    assert_eq!(f.grid.insert_lines(1), None);
    assert_eq!(f.screen_text(), before);
    f.check();
}

#[test]
fn il_and_dl_inside_the_region_scroll_from_the_cursor_row() {
    let mut f = fixture(5, 10);
    f.label_rows();
    f.goto(1, 0);
    assert!(f.grid.insert_lines(2).is_some());
    f.check();
    assert_eq!(f.screen_text(), "r0|||r1|r2");
    assert!(f.grid.delete_lines(1).is_some());
    f.check();
    assert_eq!(f.screen_text(), "r0||r1|r2|");
}

#[test]
fn backspace_clears_pending_wrap() {
    let mut f = fixture(2, 3);
    f.print("abc");
    assert!(screen(&f).cursor().pending_wrap);
    f.grid.screen_mut().backspace();
    assert!(!screen(&f).cursor().pending_wrap);
    assert_eq!(screen(&f).cursor().pos.col, 1);
}

#[test]
fn backspace_at_col_zero_keeps_pending_wrap() {
    let mut f = fixture(2, 1);
    f.print("a");
    assert!(screen(&f).cursor().pending_wrap);
    f.grid.screen_mut().backspace();
    assert!(screen(&f).cursor().pending_wrap);
    assert_eq!(screen(&f).cursor().pos.col, 0);
}

#[test]
fn carriage_return_clears_pending_wrap() {
    let mut f = fixture(2, 3);
    f.print("abc");
    f.grid.screen_mut().carriage_return();
    assert!(!screen(&f).cursor().pending_wrap);
    assert_eq!(screen(&f).cursor().pos.col, 0);
}

#[test]
fn save_restore_cursor_round_trip() {
    let mut f = fixture(5, 10);
    f.goto(2, 3);
    f.set_style(bg_pen());
    f.grid.screen_mut().save_cursor();
    let saved = *screen(&f).cursor();
    f.goto(0, 0);
    f.set_style(Style::DEFAULT);
    f.grid.screen_mut().restore_cursor();
    assert_eq!(*screen(&f).cursor(), saved);
    assert_eq!(*screen(&f).saved_cursor(), saved);
    f.check();
}

#[test]
fn restore_without_a_save_homes_the_cursor() {
    let mut f = fixture(5, 10);
    f.goto(2, 3);
    f.grid.screen_mut().restore_cursor();
    assert_eq!(
        screen(&f).cursor().pos,
        Pos {
            row: screen(&f).screen_top(),
            col: 0
        }
    );
}

#[test]
fn pen_background_becomes_the_erase_cell() {
    let mut f = fixture(2, 4);
    f.set_style(bg_pen());
    let erase = screen(&f).cursor().erase();
    let style = f.interner.resolve_style(erase.style_id());
    assert_eq!(style.bg, Color::Palette(4));
    assert_eq!(style.fg, Style::DEFAULT.fg);
    assert_eq!(style.attrs, Attrs::empty());
    f.set_style(Style::DEFAULT);
    assert_eq!(screen(&f).cursor().erase(), Cell::EMPTY);
}

#[test]
fn tab_stops_every_eight_columns() {
    let mut tabs = TabStops::new(20);
    assert_eq!(tabs.next_after(0), 8);
    assert_eq!(tabs.next_after(8), 16);
    assert_eq!(tabs.next_after(16), 19);
    tabs.clear(8);
    assert_eq!(tabs.next_after(0), 16);
    tabs.set(3);
    assert!(tabs.is_stop(3));
    assert_eq!(tabs.next_after(0), 3);
    tabs.clear_all();
    assert_eq!(tabs.next_after(0), 19);
    tabs.clear(500);
    tabs.set(500);
    assert!(!tabs.is_stop(500));
}

// scrolling

#[test]
fn scroll_up_feeds_history_only_from_row_zero() {
    let mut f = fixture(5, 10);
    f.label_rows();
    let report = scroll_up(&mut f, ScrollRegion { top: 1, bottom: 4 }, 1);
    assert_eq!(screen(&f).history_len(), 0);
    assert_eq!(report.history_rows, 0);
    assert_eq!(f.screen_text(), "r0|r2|r3||r4");
    let report = scroll_up_full(&mut f, 1);
    assert_eq!(screen(&f).history_len(), 1);
    assert_eq!(report.history_rows, 1);
    assert_eq!(history_text(&f, 1).trim_end(), "r0");
}

#[test]
fn bounded_region_keeps_rows_below() {
    let mut f = fixture(5, 10);
    f.label_rows();
    scroll_up(&mut f, ScrollRegion { top: 0, bottom: 3 }, 1);
    assert_eq!(f.screen_text(), "r1|r2||r3|r4");
    assert_eq!(screen(&f).history_len(), 1);
    assert_eq!(history_text(&f, 1).trim_end(), "r0");
}

#[test]
fn small_region_rotates_then_blanks() {
    let mut f = fixture(5, 10);
    f.label_rows();
    let newest = screen(&f).newest();
    scroll_up(&mut f, ScrollRegion { top: 1, bottom: 3 }, 9);
    assert_eq!(f.screen_text(), "r0|||r3|r4");
    assert_eq!(screen(&f).newest(), newest);
    scroll_down(&mut f, ScrollRegion { top: 1, bottom: 3 }, 9);
    assert_eq!(f.screen_text(), "r0|||r3|r4");
}

#[test]
fn scroll_down_never_pulls_from_history() {
    let mut f = fixture(3, 10);
    f.label_rows();
    newlines(&mut f, 3);
    let history = screen(&f).history_len();
    scroll_down_full(&mut f, 1);
    assert_eq!(screen(&f).history_len(), history);
    assert_eq!(f.index_text(0).trim_end(), "");
}

#[test]
fn scroll_with_empty_arguments_changes_nothing() {
    let mut f = fixture(4, 10);
    f.label_rows();
    let batch = f.grid.begin_batch();
    let region = ScrollRegion { top: 2, bottom: 2 };
    assert_eq!(scroll_up(&mut f, region, 3), ScrollReport::NONE);
    assert_eq!(scroll_down_full(&mut f, 0), ScrollReport::NONE);
    let id = screen(&f).row_of_index(1);
    assert!(screen(&f).row(id).seq() < batch);
}

#[test]
fn dl_at_row_zero_feeds_history() {
    let mut f = fixture(4, 10);
    f.label_rows();
    f.goto(0, 0);
    let report = f.grid.delete_lines(1);
    f.check();
    assert_eq!(report.map(|r| r.history_rows), Some(1));
    assert_eq!(screen(&f).history_len(), 1);
    assert_eq!(f.screen_text(), "r1|r2|r3|");
}

#[test]
fn scrolling_with_a_pen_background_fills_new_rows_with_it() {
    let mut f = fixture(4, 4);
    f.label_rows();
    f.set_style(bg_pen());
    let erase = screen(&f).cursor().erase();
    scroll_up(&mut f, ScrollRegion { top: 1, bottom: 3 }, 1);
    let blank = screen(&f).row(screen(&f).row_of_index(2));
    assert_eq!(blank.cells(), &[erase; 4]);
    scroll_up_full(&mut f, 1);
    let blank = screen(&f).row(screen(&f).row_of_index(3));
    assert_eq!(blank.cells(), &[erase; 4]);
    assert_eq!(blank.occ(), 0);
}

#[test]
fn cursor_keeps_its_screen_index_through_history_scrolls() {
    let mut f = fixture(4, 10);
    f.goto(2, 5);
    let before = screen(&f).cursor().pos.row;
    scroll_up_full(&mut f, 2);
    assert_eq!(screen(&f).cursor_row_index(), 2);
    assert_eq!(screen(&f).cursor().pos.row, before + 2);
    f.grid.screen_mut().save_cursor();
    scroll_up(&mut f, ScrollRegion { top: 1, bottom: 3 }, 1);
    assert_eq!(screen(&f).cursor_row_index(), 2);
}

// print

#[test]
fn pending_wrap_then_el0_erases_nothing() {
    for autowrap in [true, false] {
        let mut f = fixture(2, 5);
        f.print_with(
            PrintMode {
                insert: false,
                autowrap,
            },
            "abcde",
        );
        assert!(screen(&f).cursor().pending_wrap);
        f.grid.screen_mut().erase_line(LineClear::Right);
        f.check();
        assert_eq!(f.index_text(0), "abcde");
    }
}

#[test]
fn autowrap_reenabled_over_pending_wrap_breaks_line() {
    let mut f = fixture(2, 4);
    f.print_with(
        PrintMode {
            insert: false,
            autowrap: false,
        },
        "abcde",
    );
    assert_eq!(f.index_text(0), "abce");
    f.print("X");
    assert_eq!(f.index_text(1), "X   ");
    let cursor = screen(&f).cursor();
    assert_eq!((screen(&f).cursor_row_index(), cursor.pos.col), (1, 1));
    assert!(screen(&f).row(screen(&f).row_of_index(0)).wrapped());
}

#[test]
fn printing_at_the_bottom_row_scrolls_when_wrapping() {
    let mut f = fixture(2, 3);
    f.print("abcdefgh");
    assert_eq!(screen(&f).history_len(), 1);
    assert_eq!(f.screen_text(), "def|gh");
}

#[test]
fn tab_with_pending_wrap_wraps_and_is_consumed() {
    let mut f = fixture(3, 4);
    f.print("abcd");
    f.grid.put_tab(1, PrintMode::default());
    f.check();
    let cursor = screen(&f).cursor();
    assert_eq!((screen(&f).cursor_row_index(), cursor.pos.col), (1, 0));
    assert!(!cursor.pending_wrap);
}

#[test]
fn tab_with_autowrap_off_is_consumed_in_place() {
    let mut f = fixture(3, 4);
    let mode = PrintMode {
        insert: false,
        autowrap: false,
    };
    f.print_with(mode, "abcd");
    f.grid.put_tab(1, mode);
    f.check();
    let cursor = screen(&f).cursor();
    assert_eq!((screen(&f).cursor_row_index(), cursor.pos.col), (0, 3));
    assert!(cursor.pending_wrap);
    assert_eq!(f.index_text(0), "abcd");
}

#[test]
fn tab_moves_to_stops_and_marks_blank_cells() {
    let mut f = fixture(2, 20);
    f.grid.put_tab(1, PrintMode::default());
    f.check();
    assert_eq!(screen(&f).cursor().pos.col, 8);
    f.grid.put_tab(5, PrintMode::default());
    f.check();
    assert_eq!(screen(&f).cursor().pos.col, 19);
    assert_eq!(&f.index_text(0)[..1], "\t");
    assert_eq!(f.index_text(0).matches('\t').count(), 4);
    f.grid.screen_mut().tabs_mut().clear(8);
    f.goto(1, 0);
    f.grid.put_tab(1, PrintMode::default());
    assert_eq!(screen(&f).cursor().pos.col, 16);
}

#[test]
fn tab_keeps_a_spacer_a_spacer() {
    let mut f = fixture(2, 6);
    f.print(WIDE_A);
    f.goto(0, 1);
    f.grid.put_tab(1, PrintMode::default());
    f.check();
    let spacer = screen(&f).row(screen(&f).row_of_index(0)).cell(1);
    assert_eq!(spacer.width(), CellWidth::WideSpacer);
    assert_eq!(spacer.content(), CellContent::Scalar('\t'));
}

#[test]
fn wide_at_last_column() {
    let mut f = fixture(3, 4);
    f.goto(0, 3);
    f.print(WIDE_A);
    let top = screen(&f).row(screen(&f).row_of_index(0));
    assert_eq!(top.cell(3).width(), CellWidth::LeadingWideSpacer);
    assert!(top.wrapped());
    let next = screen(&f).row(screen(&f).row_of_index(1));
    assert_eq!(next.cell(0).width(), CellWidth::Wide);
    assert_eq!(next.cell(1).width(), CellWidth::WideSpacer);
    assert_eq!(screen(&f).cursor().pos.col, 2);

    let mut f = fixture(3, 4);
    f.goto(0, 3);
    f.print_with(
        PrintMode {
            insert: false,
            autowrap: false,
        },
        WIDE_A,
    );
    assert_eq!(screen(&f).dropped_wide(), 1);
    let top = screen(&f).row(screen(&f).row_of_index(0));
    assert!(!top.wrapped());
    assert_eq!(top.cell(3), Cell::EMPTY);
    assert!(screen(&f).cursor().pending_wrap);
}

#[test]
fn wide_wrapping_over_a_pair() {
    let mut f = fixture(3, 4);
    f.goto(1, 0);
    f.print(WIDE_A);
    f.goto(0, 3);
    f.print(WIDE_B);
    assert_eq!(f.index_text(1), "\u{FF22}  ");
}

#[test]
fn wide_wrapping_over_a_pair_keeps_its_filler() {
    let mut f = fixture(3, 4);
    f.goto(1, 0);
    f.print(WIDE_A);
    f.goto(0, 3);
    f.print(WIDE_B);
    let top = screen(&f).row(screen(&f).row_of_index(0));
    assert_eq!(top.cell(3).width(), CellWidth::LeadingWideSpacer);
    assert!(top.wrapped());
    let next = screen(&f).row(screen(&f).row_of_index(1));
    assert_eq!(next.cell(0).width(), CellWidth::Wide);
    assert_eq!(next.cell(0).content(), CellContent::Scalar('\u{FF22}'));
    assert_eq!(next.cell(1).width(), CellWidth::WideSpacer);
}

#[test]
fn insert_mode_moving_a_wrapped_wide_releases_its_filler() {
    let mut f = fixture(3, 4);
    f.goto(0, 3);
    f.print(WIDE_A);
    f.goto(1, 0);
    f.print_with(
        PrintMode {
            insert: true,
            autowrap: true,
        },
        "n",
    );
    let top = screen(&f).row(screen(&f).row_of_index(0));
    assert_eq!(top.cell(3).width(), CellWidth::Narrow);
    assert!(top.wrapped());
    assert_eq!(f.index_text(1), "n\u{FF21} ");
}

#[test]
fn cross_row_spacer_release_keeps_wrapped() {
    for col in [0, 1] {
        let mut f = fixture(3, 4);
        f.goto(0, 3);
        f.print(WIDE_A);
        f.goto(1, col);
        f.print("n");
        let top = screen(&f).row(screen(&f).row_of_index(0));
        assert_eq!(top.cell(3).width(), CellWidth::Narrow);
        assert!(top.wrapped());
    }
}

#[test]
fn insert_mode_over_wide_repairs() {
    let mut f = fixture(2, 6);
    f.print(WIDE_A);
    f.goto(0, 1);
    f.print_with(
        PrintMode {
            insert: true,
            autowrap: true,
        },
        "n",
    );
    assert_eq!(f.index_text(0), " n    ");
    f.goto(0, 0);
    f.print_with(
        PrintMode {
            insert: true,
            autowrap: true,
        },
        "mm",
    );
    assert_eq!(f.index_text(0), "mm n  ");
}

#[test]
fn insert_mode_wide_shifts_two_columns() {
    let mut f = fixture(2, 6);
    f.print("abcd");
    f.goto(0, 1);
    f.print_with(
        PrintMode {
            insert: true,
            autowrap: true,
        },
        WIDE_A,
    );
    assert_eq!(f.index_text(0), "a\u{FF21}bcd");
}

#[test]
fn zero_width_at_col_zero_attaches_there() {
    let mut f = fixture(2, 5);
    f.print("\u{301}");
    assert_eq!(f.index_text(0), " \u{301}    ");
    assert_eq!(screen(&f).cursor().pos.col, 0);
}

#[test]
fn zero_width_attaches_to_the_glyph_before_the_cursor() {
    let mut f = fixture(2, 5);
    f.print("ae\u{301}\u{302}");
    assert_eq!(f.index_text(0), "ae\u{301}\u{302}   ");
    assert_eq!(screen(&f).cursor().pos.col, 2);
    f.print(WIDE_A);
    f.print("\u{301}");
    assert_eq!(f.index_text(0), "ae\u{301}\u{302}\u{FF21}\u{301} ");
}

#[test]
fn zero_width_with_pending_wrap_attaches_to_the_last_cell() {
    let mut f = fixture(2, 3);
    f.print("abc");
    f.print("\u{301}");
    assert_eq!(f.index_text(0), "ab\u{63}\u{301}");
    assert!(screen(&f).cursor().pending_wrap);
}

#[test]
fn wide_dropped_when_one_column() {
    let mut f = fixture(2, 1);
    f.print(WIDE_A);
    assert_eq!(screen(&f).dropped_wide(), 1);
    assert_eq!(f.index_text(0), " ");
    assert_eq!(screen(&f).cursor().pos.col, 0);
}

#[test]
fn control_chars_are_ignored() {
    let mut f = fixture(2, 4);
    f.print("a");
    for c in ['\n', '\u{7}', '\u{1b}', '\u{85}', '\0'] {
        f.grid.print(c, 1, PrintMode::default(), &mut f.interner);
        f.grid.print(c, 0, PrintMode::default(), &mut f.interner);
    }
    f.check();
    assert_eq!(f.index_text(0), "a   ");
    assert_eq!(screen(&f).cursor().pos.col, 1);
}

#[test]
fn glyphs_inherit_the_pen() {
    let mut f = fixture(2, 4);
    f.set_style(bg_pen());
    f.print("a");
    let cell = screen(&f).row(screen(&f).row_of_index(0)).cell(0);
    assert_eq!(*f.interner.resolve_style(cell.style_id()), bg_pen());
    assert_eq!(cell.content(), CellContent::Scalar('a'));
}

// erase

#[test]
fn ed2_scrolls_occupied_rows_into_history() {
    let mut f = fixture(4, 10);
    f.print("hello");
    let report = erase_display(&mut f, DisplayClear::All).expect("primary gives a report");
    assert_eq!(report.history_rows, 1);
    assert_eq!(screen(&f).history_len(), 1);
    assert!(history_text(&f, 4).starts_with("hello"));
    assert_eq!(f.screen_text(), "|||");
    assert_eq!(screen(&f).cursor_row_index(), 0);
}

#[test]
fn ed2_of_a_blank_screen_adds_no_history() {
    let mut f = fixture(4, 10);
    let report = erase_display(&mut f, DisplayClear::All).expect("primary gives a report");
    assert_eq!(report, ScrollReport::NONE);
    assert_eq!(screen(&f).history_len(), 0);
}

#[test]
fn ed2_treats_styled_and_inverse_blanks_as_content() {
    let mut f = fixture(4, 4);
    f.goto(1, 0);
    f.set_style(Style {
        attrs: Attrs::INVERSE,
        ..Style::DEFAULT
    });
    f.print(" ");
    erase_display(&mut f, DisplayClear::All);
    assert_eq!(screen(&f).history_len(), 2);
}

#[test]
fn ed2_keeps_scrolled_back_view() {
    let mut f = fixture(4, 10);
    f.label_rows();
    newlines(&mut f, 6);
    f.print("tail");
    f.grid.screen_mut().scroll_viewport(-2);
    let top = screen(&f).visible_top();
    erase_display(&mut f, DisplayClear::All);
    assert_eq!(screen(&f).visible_top(), top);
    assert!(screen(&f).scroll_offset() > 2);
}

#[test]
fn ed2_keeps_image_rows() {
    let mut f = fixture(5, 10);
    let extras = f.interner.extras(Extras {
        hyperlink: None,
        graphic: Some(GraphicId(1)),
    });
    let id = screen(&f).row_of_index(2);
    let image = Cell::EMPTY.with_extras(extras);
    f.grid
        .screen_mut()
        .row_mut(id)
        .expect("live row")
        .set(0, image);
    f.check();
    erase_display(&mut f, DisplayClear::All);
    assert_eq!(screen(&f).history_len(), 3);
}

#[test]
fn ed2_alt_blanks_without_history() {
    let mut f = fixture(4, 10);
    f.grid.enter_alt();
    f.check();
    f.print("hello");
    assert_eq!(erase_display(&mut f, DisplayClear::All), None);
    assert_eq!(screen(&f).history_len(), 0);
    assert_eq!(f.screen_text(), "|||");
}

#[test]
fn ed0_clears_from_the_cursor_to_the_end() {
    let mut f = fixture(3, 6);
    f.label_rows();
    f.goto(1, 1);
    erase_display(&mut f, DisplayClear::Below);
    assert_eq!(f.screen_text(), "r0|r|");
}

#[test]
fn ed1_clears_row_zero() {
    let mut f = fixture(3, 10);
    f.print("aabbb");
    f.goto(0, 1);
    erase_display(&mut f, DisplayClear::Above);
    assert_eq!(f.index_text(0), "  bbb     ");
}

#[test]
fn ed1_resets_rows_above() {
    let mut f = fixture(3, 6);
    f.label_rows();
    f.goto(2, 0);
    erase_display(&mut f, DisplayClear::Above);
    assert_eq!(f.screen_text(), "|| 2");
}

#[test]
fn ed3_resets_offset_and_reports_trimmed() {
    let mut f = fixture(3, 10);
    newlines(&mut f, 8);
    f.grid.screen_mut().scroll_viewport(-3);
    let top = screen(&f).screen_top();
    let report = erase_display(&mut f, DisplayClear::Saved).expect("history was not empty");
    assert_eq!(report.trimmed, Some(top));
    assert_eq!(screen(&f).scroll_offset(), 0);
    assert_eq!(screen(&f).history_len(), 0);
    assert_eq!(screen(&f).oldest(), top);
    assert_eq!(erase_display(&mut f, DisplayClear::Saved), None);
}

#[test]
fn erased_cells_keep_only_background() {
    let mut f = fixture(3, 8);
    f.print("abcdefgh");
    f.set_style(bg_pen());
    let erase = screen(&f).cursor().erase();
    assert_ne!(erase, Cell::EMPTY);
    let cells = |f: &Fixture, index: u16| {
        f.grid
            .screen()
            .row(f.grid.screen().row_of_index(index))
            .cells()
            .to_vec()
    };

    f.goto(0, 1);
    f.grid.screen_mut().erase_chars(2);
    f.check();
    assert_eq!(cells(&f, 0)[1..3], [erase; 2]);

    f.grid.screen_mut().delete_chars(1);
    f.check();
    assert_eq!(cells(&f, 0)[7], erase);

    f.grid.screen_mut().insert_blanks(1);
    f.check();
    assert_eq!(cells(&f, 0)[1], erase);

    f.goto(1, 0);
    f.print("xyz");
    f.grid.screen_mut().erase_line(LineClear::All);
    f.check();
    assert_eq!(cells(&f, 1), [erase; 8]);

    f.goto(0, 0);
    erase_display(&mut f, DisplayClear::Below);
    assert_eq!(cells(&f, 0), [erase; 8]);
    assert_eq!(cells(&f, 2), [erase; 8]);
}

#[test]
fn erase_line_left_and_right() {
    let mut f = fixture(2, 6);
    f.print("abcdef");
    f.goto(0, 2);
    f.grid.screen_mut().erase_line(LineClear::Left);
    f.check();
    assert_eq!(f.index_text(0), "   def");
    f.goto(0, 4);
    f.grid.screen_mut().erase_line(LineClear::Right);
    f.check();
    assert_eq!(f.index_text(0), "   d  ");
}

#[test]
fn erasing_an_unwritten_row_allocates_nothing() {
    let mut f = fixture(3, 6);
    f.goto(1, 2);
    f.grid.screen_mut().erase_line(LineClear::All);
    f.grid.screen_mut().erase_chars(3);
    f.grid.screen_mut().delete_chars(3);
    f.grid.screen_mut().insert_blanks(3);
    erase_display(&mut f, DisplayClear::Below);
    f.grid.screen_mut().erase_chars(0);
    assert!(screen(&f).slots.iter().all(Option::is_none));
}

#[test]
fn in_row_edits_with_a_zero_count_stamp_nothing() {
    let mut f = fixture(2, 6);
    f.print("abc");
    let batch = f.grid.begin_batch();
    f.grid.screen_mut().erase_chars(0);
    f.grid.screen_mut().delete_chars(0);
    f.grid.screen_mut().insert_blanks(0);
    assert!(screen(&f).row(screen(&f).row_of_index(0)).seq() < batch);
}

#[test]
fn dch_and_ich_on_the_screen() {
    let mut f = fixture(2, 8);
    f.print("abcdefgh");
    f.goto(0, 2);
    f.grid.screen_mut().delete_chars(3);
    f.check();
    assert_eq!(f.index_text(0), "abfgh   ");
    f.grid.screen_mut().insert_blanks(2);
    f.check();
    assert_eq!(f.index_text(0), "ab  fgh ");
}

#[test]
fn wipe_blanks_the_screen_and_follows_output() {
    let mut f = fixture(3, 6);
    f.label_rows();
    newlines(&mut f, 4);
    f.grid.screen_mut().scroll_viewport(-2);
    f.grid.wipe();
    f.check();
    assert_eq!(f.screen_text(), "||");
    assert_eq!(screen(&f).scroll_offset(), 0);
    assert!(screen(&f).history_len() > 0);
}

#[test]
fn reset_restores_the_power_on_state_but_not_ids() {
    let mut f = fixture(4, 20);
    f.label_rows();
    newlines(&mut f, 6);
    f.grid.screen_mut().set_region(1, 3, CursorOrigin::Screen);
    f.grid.screen_mut().tabs_mut().clear_all();
    f.set_style(bg_pen());
    let newest = screen(&f).newest();
    f.grid.reset();
    f.check();
    assert_eq!(screen(&f).newest(), newest);
    assert_eq!(screen(&f).history_len(), 0);
    assert_eq!(screen(&f).region(), ScrollRegion::full(4));
    assert_eq!(screen(&f).cursor().erase(), Cell::EMPTY);
    assert_eq!(screen(&f).cursor().pos.row, screen(&f).screen_top());
    assert!(screen(&f).tabs.is_stop(8));
    assert_eq!(f.screen_text(), "|||");
}

#[test]
fn row_text_skips_spacers_and_resolves_clusters() {
    let mut f = fixture(2, 8);
    f.print(&format!("a{WIDE_A}e\u{301}"));
    assert_eq!(f.index_text(0), format!("a{WIDE_A}e\u{301}    "));
    let blank = f.row_text(RowId(99));
    assert_eq!(blank, " ".repeat(8));
}

#[test]
fn debug_output_is_a_summary() {
    let f = fixture_with(3, 5, 10_000);
    let text = format!("{:?}", screen(&f));
    assert!(text.len() < 400, "{text}");
}

// damage

#[test]
fn blanking_stamps_dirty_with_batch_seq() {
    let mut f = fixture(5, 10);
    f.label_rows();
    let allocated_screen_rows = |f: &Fixture| {
        let screen = f.grid.screen();
        (0..screen.rows())
            .map(|index| screen.row(screen.row_of_index(index)))
            .filter(|row| row.is_allocated())
            .map(|row| (row.seq(), row.flags()))
            .collect::<Vec<_>>()
    };

    let batch = f.grid.begin_batch();
    scroll_up(&mut f, ScrollRegion { top: 1, bottom: 4 }, 1);
    let stamps = allocated_screen_rows(&f);
    assert!(
        stamps[1..4]
            .iter()
            .all(|&(seq, flags)| seq == batch && flags.contains(RowFlags::DIRTY))
    );

    let batch = f.grid.begin_batch();
    scroll_up(&mut f, ScrollRegion { top: 0, bottom: 3 }, 1);
    let stamps = allocated_screen_rows(&f);
    assert!(
        stamps[2..].iter().all(|&(seq, _)| seq == batch),
        "{stamps:?}"
    );

    let batch = f.grid.begin_batch();
    erase_display(&mut f, DisplayClear::All);
    assert!(
        allocated_screen_rows(&f)
            .iter()
            .all(|&(seq, _)| seq == batch)
    );

    f.label_rows();
    let batch = f.grid.begin_batch();
    f.grid.reset();
    assert!(
        allocated_screen_rows(&f)
            .iter()
            .all(|&(seq, _)| seq == batch)
    );
}

#[test]
fn seq_starts_at_one_and_grows() {
    let mut f = fixture(2, 4);
    f.print("a");
    let first = screen(&f).row(screen(&f).row_of_index(0)).seq();
    assert_eq!(first, SeqNo(1));
    assert_eq!(f.grid.begin_batch(), SeqNo(2));
    assert_eq!(f.grid.begin_batch(), SeqNo(3));
    f.print("b");
    assert_eq!(screen(&f).row(screen(&f).row_of_index(0)).seq(), SeqNo(3));
}

// memory

#[test]
fn empty_scrollback_costs_only_its_ring() {
    let mut f = fixture_with(45, 160, 10_000);
    f.grid.screen_mut().goto(0, 0);
    for _ in 0..10_045 {
        f.grid.linefeed();
    }
    let screen = f.grid.screen();
    assert_eq!(screen.history_len(), 10_000);
    assert!(screen.slots.iter().all(Option::is_none));
    assert_eq!(size_of::<Option<Row>>(), size_of::<Row>());
    let ring = screen.ring_len() * size_of::<Option<Row>>();
    assert!((ring..ring + 4096).contains(&screen.heap_bytes()));
}

/// Where every row buffer and the ring itself live.
fn allocations(screen: &Screen) -> (BTreeSet<usize>, usize) {
    let buffers = screen
        .slots
        .iter()
        .flatten()
        .chain(screen.spare.as_ref())
        .map(|row| row.cells().as_ptr() as usize)
        .collect();
    (buffers, screen.slots.as_ptr() as usize)
}

fn assert_steady(f: &Fixture, bytes: usize, buffers: &BTreeSet<usize>, ring: usize) {
    let screen = f.grid.screen();
    assert_eq!(screen.heap_bytes(), bytes);
    let (now, now_ring) = allocations(screen);
    assert!(now.is_subset(buffers), "a row buffer was allocated");
    assert_eq!(now_ring, ring);
}

#[test]
fn no_allocation_when_scrolling_full_history() {
    for pen in [Style::DEFAULT, bg_pen()] {
        let mut f = fixture_with(4, 10, 8);
        f.set_style(pen);
        let cycle = |f: &mut Fixture| {
            f.goto(3, 0);
            f.print("x");
            f.newline();
        };
        for _ in 0..20 {
            cycle(&mut f);
        }
        let bytes = f.grid.screen().heap_bytes();
        let (buffers, ring) = allocations(f.grid.screen());
        for _ in 0..1000 {
            cycle(&mut f);
            assert_steady(&f, bytes, &buffers, ring);
        }
    }
}

#[test]
fn no_allocation_when_rotating_a_region() {
    let mut f = fixture_with(6, 10, 8);
    f.label_rows();
    let region = ScrollRegion { top: 1, bottom: 5 };
    let bytes = f.grid.screen().heap_bytes();
    let (buffers, ring) = allocations(f.grid.screen());
    for _ in 0..100 {
        scroll_up(&mut f, region, 1);
        assert_steady(&f, bytes, &buffers, ring);
        scroll_down(&mut f, region, 1);
        assert_steady(&f, bytes, &buffers, ring);
    }
}

// checker tests: the walk must not be vacuous

fn plain_screen() -> (Screen, Anchors) {
    let mut anchors = Anchors::new();
    let screen = Screen::new(
        ScreenKind::Primary,
        Size { rows: 4, cols: 6 },
        100,
        &mut anchors,
    );
    (screen, anchors)
}

#[test]
fn check_reports_a_row_with_the_wrong_id_in_a_live_slot() {
    let (mut screen, _) = plain_screen();
    let id = screen.row_of_index(1);
    let index = screen.slot_index(id);
    screen.slots[index] = Some(Row::new(RowId(900), 6, SeqNo(1), Cell::EMPTY));
    assert_eq!(
        screen.check_rows(None),
        Err(IntegrityViolation::RowIdMismatch { id })
    );
}

#[test]
fn check_reports_an_offset_past_the_history() {
    let (mut screen, _) = plain_screen();
    screen.offset = 3;
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::OffsetPastHistory)
    );
}

#[test]
fn check_reports_a_cursor_below_the_newest_row() {
    let (mut screen, _) = plain_screen();
    screen.cursor.pos.row = screen.newest() + 1;
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::CursorOutside { saved: false })
    );
    let (mut screen, _) = plain_screen();
    screen.saved_cursor.pos.col = 6;
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::CursorOutside { saved: true })
    );
}

#[test]
fn check_reports_bad_ids_history_ring_lane_and_region() {
    let (mut screen, _) = plain_screen();
    screen.oldest = screen.newest + 1;
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::IdsOutOfOrder)
    );

    let (mut screen, _) = plain_screen();
    screen.newest = RowId(1);
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::TooFewLiveRows)
    );

    let (mut screen, _) = plain_screen();
    screen.newest = RowId(screen.slots.len() as u64 + 3);
    screen.newest_issued = screen.newest;
    assert_eq!(screen.check_bounds(), Err(IntegrityViolation::RingOverflow));

    let (mut screen, _) = plain_screen();
    screen.scrollback_limit = 0;
    screen.oldest = RowId(0);
    screen.newest = RowId(10);
    screen.newest_issued = screen.newest;
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::HistoryOverLimit)
    );

    let (mut screen, _) = plain_screen();
    screen.newest = RowId::ALT_ORIGIN + 5;
    screen.newest_issued = screen.newest;
    screen.oldest = RowId::ALT_ORIGIN + 2;
    assert_eq!(screen.check_bounds(), Err(IntegrityViolation::LaneOverlap));

    let (mut screen, _) = plain_screen();
    screen.region = ScrollRegion { top: 2, bottom: 9 };
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::RegionInvalid)
    );
}

#[test]
fn check_reports_pens_unknown_to_the_interner() {
    let (mut screen, _) = plain_screen();
    screen.cursor.template = Cell::EMPTY.with_style(crate::intern::StyleId(9));
    let want = IntegrityViolation::UnknownStyle {
        id: screen.cursor.pos.row,
        col: 0,
    };
    assert_eq!(screen.check_rows(Some(&Interner::default())), Err(want));
}

#[test]
fn check_reports_a_walked_row_of_the_wrong_width() {
    let (mut screen, _) = plain_screen();
    let id = screen.row_of_index(2);
    let index = screen.slot_index(id);
    screen.slots[index] = Some(Row::new(id, 5, SeqNo(1), Cell::EMPTY));
    assert_eq!(
        screen.check_rows(None),
        Err(IntegrityViolation::RowWidth { id })
    );
}

#[cfg(feature = "vt-paranoid")]
#[test]
fn paranoid_walk_reports_a_stale_row() {
    let (mut screen, _) = plain_screen();
    screen.slots[100] = Some(Row::new(RowId(100), 6, SeqNo(1), Cell::EMPTY));
    assert_eq!(
        screen.check_rows(None),
        Err(IntegrityViolation::StaleSlot { slot: 100 })
    );
}

#[cfg(feature = "vt-paranoid")]
#[test]
fn paranoid_walk_starts_at_the_oldest_row() {
    let mut f = fixture(3, 6);
    newlines(&mut f, 30);
    let screen = f.grid.screen();
    assert_eq!(screen.walk_start(), screen.oldest());
}

#[cfg(not(feature = "vt-paranoid"))]
#[test]
fn bounded_walk_starts_near_the_batch() {
    let mut f = fixture(3, 6);
    f.print("x");
    newlines(&mut f, 30);
    f.grid.begin_batch();
    let screen = f.grid.screen();
    assert_eq!(screen.walk_start(), screen.screen_top() - 1);
    f.grid.screen_mut().scroll_viewport(-5);
    let screen = f.grid.screen();
    assert_eq!(screen.walk_start(), screen.visible_top() - 1);
}

#[test]
fn check_reports_newest_past_newest_issued() {
    let (mut screen, _) = plain_screen();
    assert_eq!(screen.check_bounds(), Ok(()));
    screen.newest_issued = screen.newest - 1;
    assert_eq!(
        screen.check_bounds(),
        Err(IntegrityViolation::IdsOutOfOrder)
    );
}
