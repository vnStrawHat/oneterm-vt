use super::*;
use crate::cell::{Cell, Color, Style};
use crate::grid::anchor::{AnchorId, AnchorKind};
use crate::grid::grid_fixture::{Fixture, fixture, fixture_with};
use crate::grid::screen::CursorOrigin;
use crate::grid::{Pos, Row, RowsScrolled, SeqNo};

fn mark_at(f: &mut Fixture, index: u16) -> AnchorId {
    let row = f.grid.screen().row_of_index(index);
    f.grid
        .anchors
        .register(AnchorKind::Mark(7), Pos { row, col: 0 })
}

/// The screen index of an anchor, `None` when it is dead.
fn index_of(f: &Fixture, id: AnchorId) -> Option<u16> {
    let pos = f.grid.anchors.get(id)?;
    f.grid.screen_of(pos.row).index_of(pos.row)
}

fn up(f: &mut Fixture, top: u16, bottom: u16, n: u16) -> ScrollReport {
    let report = f.grid.scroll_up(ScrollRegion { top, bottom }, n);
    f.check();
    report
}

fn down(f: &mut Fixture, top: u16, bottom: u16, n: u16) -> ScrollReport {
    let report = f.grid.scroll_down(ScrollRegion { top, bottom }, n);
    f.check();
    report
}

// anchors and scrolling

#[test]
fn region_scroll_moves_anchors_with_content() {
    let mut f = fixture(6, 10);
    let inside = mark_at(&mut f, 3);
    let below = mark_at(&mut f, 5);
    up(&mut f, 1, 5, 2);
    assert_eq!(index_of(&f, inside), Some(1));
    assert_eq!(index_of(&f, below), Some(5));
}

#[test]
fn scrolled_out_anchor_dies() {
    let mut f = fixture(6, 10);
    let top = mark_at(&mut f, 1);
    up(&mut f, 1, 5, 1);
    assert_eq!(f.grid.anchors.get(top), None);

    let bottom = mark_at(&mut f, 4);
    down(&mut f, 1, 5, 1);
    assert_eq!(f.grid.anchors.get(bottom), None);
}

#[test]
fn scroll_down_moves_anchors_with_content() {
    let mut f = fixture(6, 10);
    let inside = mark_at(&mut f, 2);
    down(&mut f, 1, 5, 2);
    assert_eq!(index_of(&f, inside), Some(4));
}

#[test]
fn trim_kills_anchors() {
    let mut f = fixture_with(4, 10, 100);
    let id = mark_at(&mut f, 0);
    for _ in 0..200 {
        f.grid.scroll_up(ScrollRegion::full(4), 1);
    }
    f.check();
    assert_eq!(f.grid.anchors.get(id), None);
}

#[test]
fn history_anchors_survive_until_trimmed() {
    let mut f = fixture_with(4, 10, 100);
    let id = mark_at(&mut f, 0);
    up(&mut f, 0, 4, 3);
    let pos = f.grid.anchors.get(id).expect("still in history");
    assert!(pos.row < f.grid.screen().screen_top());
}

#[test]
fn rows_scrolled_matches_anchor_shift() {
    let mut f = fixture(6, 10);
    let top = f.grid.screen().screen_top();
    let moved = mark_at(&mut f, 3);
    let report = up(&mut f, 1, 5, 2);
    assert_eq!(
        report,
        ScrollReport {
            scrolled: Some(RowsScrolled {
                top: top + 1,
                bottom: top + 4,
                delta: -2
            }),
            trimmed: None,
            history_rows: 0,
        }
    );
    assert_eq!(f.grid.anchors.get(moved).map(|pos| pos.row), Some(top + 1));
    let report = down(&mut f, 1, 5, 1);
    assert_eq!(
        report.scrolled.map(|s| (s.top, s.bottom, s.delta)),
        Some((top + 1, top + 4, 1))
    );
}

#[test]
fn whole_screen_scroll_reports_nothing() {
    let mut f = fixture(6, 10);
    let id = mark_at(&mut f, 3);
    let before = f.grid.anchors.get(id);
    let report = up(&mut f, 0, 6, 2);
    assert_eq!(report.scrolled, None);
    assert_eq!(report.history_rows, 2);
    assert_eq!(f.grid.anchors.get(id), before);
    assert_eq!(index_of(&f, id), Some(1));
}

#[test]
fn bounded_top_region_reports_tail_shift() {
    let mut f = fixture(6, 10);
    let tail = mark_at(&mut f, 5);
    let before = f.grid.anchors.get(tail).expect("alive").row;
    let report = up(&mut f, 0, 4, 2);
    let newest = f.grid.screen().newest();
    assert_eq!(
        report.scrolled,
        Some(RowsScrolled {
            top: f.grid.screen().screen_top() + 4,
            bottom: newest,
            delta: 2
        })
    );
    assert_eq!(
        f.grid.anchors.get(tail).map(|pos| pos.row),
        Some(before + 2)
    );
    assert_eq!(index_of(&f, tail), Some(5));
}

#[test]
fn trim_is_reported_and_kills_only_its_lane() {
    let mut f = fixture_with(3, 10, 2);
    let primary = mark_at(&mut f, 0);
    f.grid.enter_alt();
    let alt = mark_at(&mut f, 0);
    for _ in 0..5 {
        f.grid.scroll_up(ScrollRegion::full(3), 1);
    }
    f.check();
    assert_eq!(f.grid.anchors.get(alt), None);
    assert!(f.grid.anchors.get(primary).is_some());
}

#[test]
fn alt_scroll_leaves_primary_anchors() {
    let mut f = fixture(5, 10);
    let mark = mark_at(&mut f, 2);
    let selection = f.grid.anchors.register(
        AnchorKind::SelectionStart,
        Pos {
            row: RowId(1),
            col: 4,
        },
    );
    let marks = (f.grid.anchors.get(mark), f.grid.anchors.get(selection));
    f.grid.enter_alt();
    f.check();
    for _ in 0..30 {
        f.grid.linefeed();
    }
    f.check();
    assert_eq!(
        (f.grid.anchors.get(mark), f.grid.anchors.get(selection)),
        marks
    );
    assert!(marks.0.is_some() && marks.1.is_some());
}

#[test]
fn erase_keeps_anchors_alive() {
    let mut f = fixture(4, 10);
    f.label_rows();
    let id = mark_at(&mut f, 1);
    f.goto(3, 0);
    f.grid.screen_mut().erase_line(crate::grid::LineClear::All);
    f.grid.screen_mut().erase_chars(2);
    f.grid.screen_mut().delete_chars(2);
    f.grid.screen_mut().insert_blanks(2);
    f.grid.erase_display(DisplayClear::Below, &f.interner);
    f.goto(0, 0);
    f.grid.erase_display(DisplayClear::Above, &f.interner);
    f.check();
    assert_eq!(index_of(&f, id), Some(1));
    f.grid.erase_display(DisplayClear::All, &f.interner);
    f.check();
    assert!(
        f.grid.anchors.get(id).is_some(),
        "the row keeps its id in history"
    );
    f.grid.erase_display(DisplayClear::Saved, &f.interner);
    f.check();
    assert_eq!(f.grid.anchors.get(id), None);
}

#[test]
fn wipe_kills_screen_anchors() {
    let mut f = fixture(4, 10);
    f.grid.enter_alt();
    let alt = mark_at(&mut f, 2);
    f.grid.wipe();
    f.check();
    assert_eq!(f.grid.anchors.get(alt), None);
    f.grid.leave_alt();
    let primary = mark_at(&mut f, 2);
    f.grid.wipe();
    assert_eq!(f.grid.anchors.get(primary), None);
}

#[test]
fn screen_owned_anchors_follow_their_screen() {
    let mut f = fixture(4, 10);
    f.goto(2, 5);
    up(&mut f, 0, 4, 2);
    let screen = f.grid.screen();
    assert_eq!(
        f.grid.anchors.get(screen.cursor_anchor()),
        Some(screen.cursor().pos)
    );
    f.grid.screen_mut().scroll_viewport(-1);
    f.check();
    let top = f.grid.screen().visible_top();
    let viewport = f.grid.screen().owned_anchors()[2];
    assert_eq!(
        f.grid.anchors.get(viewport.0),
        Some(Pos { row: top, col: 0 })
    );
}

#[test]
fn released_id_is_recycled() {
    let mut f = fixture(4, 10);
    let first = mark_at(&mut f, 0);
    f.grid.anchors.release(first);
    let second = mark_at(&mut f, 1);
    assert_eq!(first, second);
    f.check();
}

// alt screen

fn red_pen(f: &mut Fixture) {
    f.set_style(Style {
        bg: Color::Palette(1),
        ..Style::DEFAULT
    });
}

#[test]
fn enter_keeps_cursor_index() {
    let mut f = fixture(5, 10);
    red_pen(&mut f);
    f.goto(3, 4);
    let entry = *f.grid.screen().cursor();
    f.grid.enter_alt();
    f.check();
    assert!(f.grid.alt_active());
    let alt = f.grid.screen().cursor();
    assert_eq!((f.grid.screen().cursor_row_index(), alt.pos.col), (3, 4));
    assert_eq!(alt.erase(), entry.erase());
    assert_eq!(alt.pending_wrap, entry.pending_wrap);
    assert_eq!(alt.pos.row, f.grid.alt().row_of_index(3));
}

#[test]
fn enter_carries_a_pending_wrap() {
    let mut f = fixture(3, 4);
    f.print("abcd");
    f.grid.enter_alt();
    assert!(f.grid.screen().cursor().pending_wrap);
}

#[test]
fn leave_restores_entry_cursor() {
    let mut f = fixture(5, 10);
    f.goto(3, 4);
    let entry = *f.grid.primary().cursor();
    f.grid.enter_alt();
    f.goto(0, 0);
    f.print("alt");
    f.grid.leave_alt();
    f.check();
    assert!(!f.grid.alt_active());
    assert_eq!(*f.grid.screen().cursor(), entry);
}

#[test]
fn region_and_tabs_shared_both_ways() {
    let mut f = fixture(6, 20);
    f.grid.screen_mut().set_region(1, 4, CursorOrigin::Screen);
    f.grid.screen_mut().tabs_mut().clear(8);
    f.grid.enter_alt();
    assert_eq!(f.grid.screen().region(), ScrollRegion { top: 1, bottom: 4 });
    f.goto(0, 0);
    f.grid.put_tab(1, PrintMode::default());
    assert_eq!(f.grid.screen().cursor().pos.col, 16);
    f.grid.screen_mut().set_region(2, 5, CursorOrigin::Screen);
    f.grid.screen_mut().tabs_mut().set(8);
    f.grid.leave_alt();
    f.check();
    assert_eq!(f.grid.screen().region(), ScrollRegion { top: 2, bottom: 5 });
    f.goto(5, 0);
    f.grid.put_tab(1, PrintMode::default());
    assert_eq!(f.grid.screen().cursor().pos.col, 8);
}

#[test]
fn wipe_uses_entering_background() {
    let mut f = fixture(3, 4);
    red_pen(&mut f);
    let erase = f.grid.screen().cursor().erase();
    f.grid.enter_alt();
    f.grid.wipe();
    f.check();
    let screen = f.grid.screen();
    for index in 0..3 {
        assert_eq!(screen.row(screen.row_of_index(index)).cells(), &[erase; 4]);
    }
}

#[test]
fn enter_leave_are_idempotent() {
    let mut f = fixture(4, 8);
    f.grid.leave_alt();
    f.check();
    assert!(!f.grid.alt_active());
    f.grid.enter_alt();
    f.goto(2, 2);
    let cursor = *f.grid.screen().cursor();
    f.grid.enter_alt();
    f.check();
    assert_eq!(*f.grid.screen().cursor(), cursor);
    f.grid.leave_alt();
    f.grid.leave_alt();
    f.check();
}

#[test]
fn lanes_never_overlap() {
    let mut f = fixture(4, 8);
    for _ in 0..20 {
        f.newline();
    }
    f.grid.enter_alt();
    for _ in 0..20 {
        f.newline();
    }
    f.check();
    assert!(f.grid.primary().newest() < RowId::ALT_ORIGIN);
    assert!(f.grid.alt().oldest() >= RowId::ALT_ORIGIN);
    assert_eq!(f.grid.alt().history_len(), 0);
    assert!(std::ptr::eq(
        f.grid.screen_of(f.grid.alt().newest()),
        f.grid.alt()
    ));
    assert!(std::ptr::eq(f.grid.screen_of(RowId(5)), f.grid.primary()));
}

#[test]
fn reset_returns_to_the_primary_screen() {
    let mut f = fixture(4, 8);
    f.grid.enter_alt();
    f.print("x");
    f.grid.reset();
    f.check();
    assert!(!f.grid.alt_active());
    assert_eq!(
        f.grid.alt().row(f.grid.alt().screen_top()).cell(0),
        Cell::EMPTY
    );
}

#[test]
fn wrapline_acts_on_the_active_screen() {
    let mut f = fixture(4, 8);
    f.grid.enter_alt();
    f.grid.wrapline();
    f.check();
    assert_eq!(f.grid.alt().cursor_row_index(), 1);
    assert_eq!(f.grid.primary().cursor_row_index(), 0);
}

#[test]
fn begin_batch_stamps_both_screens() {
    let mut f = fixture(3, 8);
    assert_eq!(f.grid.begin_batch(), SeqNo(2));
    f.print("a");
    f.grid.enter_alt();
    f.print("b");
    let primary = f.grid.primary();
    let alt = f.grid.alt();
    assert_eq!(primary.row(primary.screen_top()).seq(), SeqNo(2));
    assert_eq!(alt.row(alt.screen_top()).seq(), SeqNo(2));
}

#[test]
fn heap_bytes_cover_both_rings_and_the_anchors() {
    let f = fixture_with(4, 8, 100);
    let rings = (f.grid.primary().ring_len() + f.grid.alt().ring_len()) * size_of::<Option<Row>>();
    assert!(f.grid.heap_bytes() >= rings + f.grid.anchors.heap_bytes());
}

// the tier 3 check

#[test]
fn check_reports_an_anchor_outside_its_lane() {
    let mut f = fixture(4, 8);
    let id = f.grid.anchors.register(
        AnchorKind::Mark(1),
        Pos {
            row: RowId(500),
            col: 0,
        },
    );
    f.grid.sync_anchors();
    assert_eq!(
        f.grid.check_integrity(None),
        Err(IntegrityViolation::AnchorOutsideLane { id })
    );
}

#[test]
fn check_reports_missing_screen_owned_anchors() {
    let mut f = fixture(4, 8);
    let id = f.grid.primary().cursor_anchor();
    f.grid.anchors.release(id);
    f.grid.sync_anchors();
    assert_eq!(
        f.grid.check_integrity(None),
        Err(IntegrityViolation::ScreenOwnedAnchors {
            kind: ScreenKind::Primary,
            live: 2
        })
    );
}

#[test]
fn check_reports_stale_screen_owned_anchors() {
    let mut f = fixture(4, 8);
    f.grid.screen_mut().goto(2, 2);
    assert_eq!(
        f.grid.check_integrity(None),
        Err(IntegrityViolation::ScreenOwnedAnchors {
            kind: ScreenKind::Primary,
            live: 3
        })
    );
    f.grid.sync_anchors();
    assert_eq!(f.grid.check_integrity(None), Ok(()));
}

// extras walk

fn linked(f: &mut Fixture, n: u32) -> crate::intern::ExtrasId {
    use crate::intern::{Extras, HyperlinkId};
    f.interner.extras(Extras {
        hyperlink: Some(HyperlinkId(n)),
        graphic: None,
    })
}

fn pen(f: &mut Fixture, extras: crate::intern::ExtrasId) {
    let template = Cell::EMPTY.with_extras(extras);
    f.grid.screen_mut().set_template(template, &mut f.interner);
}

#[test]
fn live_extras_ids_reads_history_and_templates() {
    use crate::intern::ExtrasId;
    use rustc_hash::FxHashSet;

    let mut f = fixture_with(3, 10, 100);
    let ids: Vec<ExtrasId> = (1..=6).map(|n| linked(&mut f, n)).collect();
    // A cell that scrolled into history.
    pen(&mut f, ids[0]);
    f.print("a");
    pen(&mut f, ExtrasId::NONE);
    for _ in 0..4 {
        f.newline();
    }
    assert!(f.grid.primary().history_len() >= 1);
    // A cell on the screen, a saved cursor, the pen of the primary screen.
    pen(&mut f, ids[1]);
    f.print("b");
    pen(&mut f, ids[3]);
    f.grid.screen_mut().save_cursor();
    pen(&mut f, ids[2]);
    // The pen of the alternate screen.
    f.grid.enter_alt();
    pen(&mut f, ids[4]);
    f.check();

    let mut live = FxHashSet::default();
    live.insert(60_000);
    f.grid.live_extras_ids(&mut live);
    for id in &ids[..5] {
        assert!(live.contains(&id.0), "{id:?} is held");
    }
    assert!(!live.contains(&ids[5].0), "an unreferenced id is not live");
    assert!(!live.contains(&60_000), "the set is replaced, not extended");
}

#[test]
fn live_extras_ids_skips_unhinted_rows() {
    use rustc_hash::FxHashSet;

    let mut f = fixture_with(3, 10, 100);
    let unused = linked(&mut f, 1);
    f.print("plain text only");
    let mut live = FxHashSet::default();
    f.grid.live_extras_ids(&mut live);
    assert!(!live.contains(&unused.0));
    assert!(
        live.iter().all(|id| *id == 0),
        "only the empty entry is held: {live:?}"
    );
}
