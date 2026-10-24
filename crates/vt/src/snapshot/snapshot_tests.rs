//! Tests of snapshot updates, copied rows, the palette and the mode snapshot. A real `Terminal`
//! is driven through `feed`; nothing reaches into grid internals.

use std::time::Instant;

use crate::cell::{Color, NamedColor, Rgb, Style};
use crate::events::{EventBatch, VtEvent};
use crate::grid::terminal_grid::ResizePolicy;
use crate::grid::{Pos, RowId, Size};
use crate::intern::{GraphicId, HyperlinkId};
use crate::selection::{SelectionKind, Side};
use crate::terminal::{Config, KeyboardFlags, Terminal};

use super::*;

/// A terminal with its batch, one snapshot state and a base instant.
pub(super) struct Session {
    pub(super) term: Terminal,
    pub(super) batch: EventBatch,
    pub(super) state: SnapshotState,
    /// The base instant: `feed` and `update` use it, `*_at` take another.
    pub(super) now: Instant,
}

impl Session {
    /// A 10 x 20 terminal.
    pub(super) fn new() -> Session {
        Session::sized(10, 20)
    }

    pub(super) fn sized(rows: u16, cols: u16) -> Session {
        Session {
            term: Terminal::new(Size { rows, cols }, Config::default()),
            batch: EventBatch::new(),
            state: SnapshotState::new(),
            now: Instant::now(),
        }
    }

    /// A terminal whose first update has been taken.
    fn settled() -> Session {
        let mut session = Session::new();
        session.update();
        session
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) {
        self.feed_at(bytes, self.now);
    }

    pub(super) fn feed_at(&mut self, bytes: &[u8], at: Instant) {
        self.term.feed(bytes, &mut self.batch, at);
    }

    pub(super) fn update(&mut self) -> SnapshotUpdate {
        self.update_at(self.now)
    }

    pub(super) fn update_at(&mut self, at: Instant) -> SnapshotUpdate {
        self.term.snapshot_update(&mut self.state, at)
    }

    /// Every `Reply` of the last feed, concatenated.
    pub(super) fn replies(&self) -> String {
        let mut text = Vec::new();
        for event in self.batch.iter() {
            if let VtEvent::Reply(span) = event {
                text.extend_from_slice(self.batch.bytes(*span));
            }
        }
        String::from_utf8_lossy(&text).into_owned()
    }

    /// Writes `row<N>` at the start of every screen row.
    fn fill_rows(&mut self) {
        let rows = self.term.size().rows;
        for index in 0..rows {
            self.feed(format!("\x1b[{};1Hrow{index}", index + 1).as_bytes());
        }
    }

    /// `count` line feeds from the last row.
    fn line_feeds(&mut self, count: usize) {
        let rows = self.term.size().rows;
        self.feed(format!("\x1b[{rows};1H{}", "\n".repeat(count)).as_bytes());
    }

    fn pos(&self, index: u16, col: u16) -> Pos {
        Pos {
            row: self.term.screen().row_of_index(index),
            col,
        }
    }

    fn row_ids(&self) -> Vec<RowId> {
        self.state.rows().iter().map(|row| row.id).collect()
    }
}

fn indices(range: std::ops::Range<u16>) -> Vec<u16> {
    range.collect()
}

fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

// ---- update decisions ----

#[test]
fn first_update_is_full() {
    let mut s = Session::new();
    assert_eq!(s.state.size(), Size { rows: 0, cols: 0 });
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert_eq!(s.state.rows().len(), 10);
    assert_eq!(s.state.changed(), indices(0..10));
    assert_eq!(s.state.rows_copied(), 10);
}

#[test]
fn idle_update_is_unchanged() {
    let mut s = Session::settled();
    let copied = s.state.rows_copied();
    for _ in 0..5 {
        assert_eq!(s.update(), SnapshotUpdate::Unchanged);
        assert!(s.state.changed().is_empty());
    }
    assert_eq!(s.state.rows_copied(), copied);
}

#[test]
fn rows_hold_the_viewport_for_every_result() {
    let mut s = Session::new();
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert_eq!(s.state.rows().len(), 10);
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
    assert_eq!(s.state.rows().len(), 10);
    s.feed(b"x");
    assert!(matches!(s.update(), SnapshotUpdate::Partial { .. }));
    assert_eq!(s.state.rows().len(), 10);
}

#[test]
fn one_written_row_is_partial() {
    let mut s = Session::settled();
    let copied = s.state.rows_copied();
    s.feed(b"\x1b[4;1Hhello");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert_eq!(s.state.changed(), [3]);
    assert_eq!(s.state.rows_copied(), copied + 1);
}

#[test]
fn one_line_scroll_is_a_move() {
    let mut s = Session::new();
    s.fill_rows();
    s.update();
    let before = s.row_ids();
    s.feed(b"\x1b[10;1H\n");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 1 });
    assert_eq!(s.state.changed(), [9]);
    assert_eq!(s.state.rows()[0].id, before[1]);
    assert_eq!(s.row_ids()[..9], before[1..]);
}

#[test]
fn two_line_scroll() {
    let mut s = Session::sized(8, 20);
    s.fill_rows();
    s.update();
    let before = s.row_ids();
    s.feed(b"\x1b[8;1H\n\n");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 2 });
    assert_eq!(s.state.changed(), [6, 7]);
    assert_eq!(s.row_ids()[..6], before[2..]);
}

#[test]
fn scroll_past_the_viewport_is_full() {
    let mut s = Session::settled();
    s.line_feeds(15);
    assert_eq!(s.update(), SnapshotUpdate::Full);
}

#[test]
fn viewport_scroll_back_is_negative() {
    let mut s = Session::new();
    s.line_feeds(20);
    s.update();
    s.term.scroll_viewport(-5);
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: -5 });
    assert_eq!(s.state.changed(), indices(0..5));
    assert_eq!(s.state.scroll_offset(), 5);
    s.term.scroll_viewport(i32::MAX);
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 5 });
    assert_eq!(s.state.changed(), indices(5..10));
    assert_eq!(s.state.scroll_offset(), 0);
}

#[test]
fn viewport_scroll_past_the_viewport_is_full() {
    let mut s = Session::new();
    s.line_feeds(30);
    s.update();
    s.term.scroll_viewport(-12);
    assert_eq!(s.update(), SnapshotUpdate::Full);
}

#[test]
fn output_while_scrolled_back_is_not_copied() {
    let mut s = Session::new();
    s.line_feeds(20);
    s.update();
    s.term.scroll_viewport(-5);
    s.update();
    s.feed(b"x");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert!(s.state.changed().is_empty());
}

#[test]
fn resize_is_full() {
    let mut s = Session::settled();
    s.term
        .resize(Size { rows: 12, cols: 20 }, ResizePolicy::BottomAnchor);
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert_eq!(s.state.rows().len(), 12);
    assert_eq!(s.state.size(), Size { rows: 12, cols: 20 });
}

#[test]
fn alt_screen_enter_and_leave_are_full() {
    let mut s = Session::settled();
    s.feed(b"\x1b[?1049h");
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert!(s.state.modes().alt_screen);
    s.feed(b"\x1b[?1049l");
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert!(!s.state.modes().alt_screen);
}

#[test]
fn ris_decaln_deccolm_reverse_video_are_full() {
    for sequence in [&b"\x1bc"[..], b"\x1b#8", b"\x1b[?3h", b"\x1b[?5h"] {
        let mut s = Session::settled();
        s.feed(sequence);
        assert_eq!(s.update(), SnapshotUpdate::Full, "{sequence:?}");
    }
    let mut s = Session::settled();
    s.feed(b"\x1b[?5h");
    s.update();
    s.feed(b"\x1b[?5h");
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
}

#[test]
fn scrollback_limit_change_is_full() {
    let mut s = Session::settled();
    s.term.set_scrollback_limit(10);
    assert_eq!(s.update(), SnapshotUpdate::Full);
}

#[test]
fn color_override_change_is_full() {
    let mut s = Session::settled();
    s.feed(b"\x1b]4;1;rgb:ff/00/00\x07");
    assert_eq!(s.update(), SnapshotUpdate::Full);
    s.feed(b"\x1b]4;1;rgb:ff/00/00\x07");
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
}

#[test]
fn theme_change_is_full() {
    let mut s = Session::settled();
    let mut theme = Palette::new();
    theme.foreground = rgb(1, 2, 3);
    s.term.set_theme_colors(&theme);
    assert_eq!(s.update(), SnapshotUpdate::Full);
    s.term.set_theme_colors(&theme);
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
}

#[test]
fn invalidate_is_full_once() {
    let mut s = Session::settled();
    s.state.invalidate();
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
}

#[test]
fn every_row_copied_is_full() {
    let mut s = Session::new();
    s.fill_rows();
    s.update();
    s.feed(b"\x1b[2J");
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert_eq!(s.state.changed(), indices(0..10));
}

#[test]
fn state_from_another_terminal_is_full() {
    let mut s = Session::settled();
    for _ in 0..50 {
        s.feed(b"x");
    }
    s.update();
    s.term = Terminal::new(Size { rows: 10, cols: 20 }, Config::default());
    assert_eq!(s.update(), SnapshotUpdate::Full);
}

#[test]
fn cursor_move_is_partial_without_rows() {
    let mut s = Session::settled();
    s.feed(b"\x1b[5;8H");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert!(s.state.changed().is_empty());
    assert_eq!(s.state.cursor().row, Some(4));
    assert_eq!(s.state.cursor().col, 7);
    assert_eq!(s.state.cursor().id, s.term.screen().row_of_index(4));
}

#[test]
fn cursor_hidden_while_scrolled_back() {
    let mut s = Session::new();
    s.line_feeds(20);
    s.update();
    assert_eq!(s.state.cursor().row, Some(9));
    s.term.scroll_viewport(-5);
    s.update();
    assert_eq!(s.state.cursor().row, None);
}

#[test]
fn selection_change_is_partial_once() {
    let mut s = Session::new();
    s.fill_rows();
    s.update();
    let (start, end) = (s.pos(0, 0), s.pos(0, 2));
    s.term
        .selection_start(start, Side::Left, SelectionKind::Simple);
    s.term.selection_update(end, Side::Right);
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert!(s.state.changed().is_empty());
    assert!(s.state.selection().is_some());
    assert_eq!(s.state.selection(), s.term.selection_range());
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
}

#[test]
fn mode_change_is_partial_without_rows() {
    let mut s = Session::settled();
    s.feed(b"\x1b[?1h");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert!(s.state.changed().is_empty());
    assert!(s.state.modes().app_cursor);
    s.feed(b"\x1b[?25l");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert!(!s.state.cursor().visible);
}

#[test]
fn insert_mode_write_is_partial() {
    let mut s = Session::settled();
    s.feed(b"\x1b[4h");
    s.update();
    s.feed(b"\x1b[3;1Hab");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert_eq!(s.state.changed(), [2]);
}

#[test]
fn region_scroll_restamps_rows() {
    let mut s = Session::new();
    s.fill_rows();
    s.update();
    s.feed(b"\x1b[2;5r\x1b[5;1H\n");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert_eq!(s.state.changed(), [1, 2, 3, 4]);
    let blank = &s.state.rows()[4];
    assert!(
        blank
            .cells
            .iter()
            .all(|cell| cell.content == SnapshotContent::Scalar(' '))
    );
}

#[test]
fn watermark_never_moves_backwards() {
    let mut s = Session::settled();
    let mut last = s.state.watermark();
    for round in 0..10 {
        s.feed(format!("line {round}\r\n").as_bytes());
        s.update();
        assert!(s.state.watermark() >= last);
        last = s.state.watermark();
    }
    s.update();
    assert_eq!(s.state.watermark(), last);
}

#[test]
fn two_states_keep_their_own_damage() {
    let mut s = Session::settled();
    let mut other = SnapshotState::new();
    s.term.snapshot_update(&mut other, s.now);
    s.feed(b"\x1b[7;1Hhi");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert_eq!(s.state.changed(), [6]);
    let second = s.term.snapshot_update(&mut other, s.now);
    assert_eq!(second, SnapshotUpdate::Partial { scrolled: 0 });
    assert_eq!(other.changed(), [6]);
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
    assert_eq!(
        s.term.snapshot_update(&mut other, s.now),
        SnapshotUpdate::Unchanged
    );
}

// ---- rows ----

#[test]
fn style_runs_carry_resolved_styles() {
    let mut s = Session::new();
    s.feed(b"\x1b[31mred");
    s.update();
    let row = &s.state.rows()[0];
    assert_eq!(row.runs.len(), 2);
    assert_eq!(row.runs[0].cols, 0..3);
    assert_eq!(row.runs[0].style.fg, Color::Named(NamedColor::Red));
    assert_eq!(row.runs[1].cols, 3..20);
    assert_eq!(row.runs[1].style, Style::DEFAULT);
    assert_eq!(row.cells[0].run, 0);
    assert_eq!(row.cells[3].run, 1);
}

#[test]
fn uniform_row_is_one_run() {
    let mut s = Session::sized(50, 200);
    s.feed(b"\x1b[42m\x1b[2J");
    s.update();
    for row in s.state.rows() {
        assert_eq!(row.runs.len(), 1);
        assert_eq!(row.runs[0].cols, 0..200);
    }
}

#[test]
fn style_of_bad_run_is_default() {
    let mut s = Session::settled();
    s.feed(b"\x1b[31mx");
    s.update();
    let row = &s.state.rows()[0];
    let mut cell = row.cells[0];
    cell.run = u16::MAX;
    assert_eq!(*row.style_of(&cell), Style::DEFAULT);
    assert_ne!(*row.style_of(&row.cells[0]), Style::DEFAULT);
}

#[test]
fn cluster_out_of_range_is_empty() {
    let row = SnapshotRow::default();
    assert_eq!(row.cluster(u32::MAX, u32::MAX), &[] as &[char]);
    assert_eq!(row.cluster(0, 1), &[] as &[char]);
}

#[test]
fn grapheme_cell_copies_as_cluster() {
    let mut s = Session::new();
    s.feed("e\u{301}x".as_bytes());
    s.update();
    let row = &s.state.rows()[0];
    let SnapshotContent::Cluster { start, len } = row.cells[0].content else {
        panic!("expected a cluster, got {:?}", row.cells[0].content);
    };
    assert_eq!(row.cluster(start, len), ['e', '\u{301}']);
    assert_eq!(row.cells[1].content, SnapshotContent::Scalar('x'));
}

#[test]
fn wrapped_row_is_flagged() {
    let mut s = Session::new();
    s.feed("a".repeat(21).as_bytes());
    s.update();
    assert!(s.state.rows()[0].wrapped);
    assert!(!s.state.rows()[1].wrapped);
}

#[test]
fn hyperlink_strings_resolved() {
    let mut s = Session::new();
    s.feed(b"\x1b]8;id=1;http://example.com/a\x07link\x1b]8;;\x07 plain");
    s.update();
    let row = &s.state.rows()[0];
    let id = row.cells[0].hyperlink.expect("the first cell is linked");
    assert_eq!(
        &*s.state.hyperlink(id).expect("the link was copied").uri,
        "http://example.com/a"
    );
    assert_eq!(
        &*s.state.hyperlink(id).expect("the link was copied").id,
        "1"
    );
    assert_eq!(row.cells[3].hyperlink, Some(id));
    assert_eq!(row.cells[5].hyperlink, None);
    assert!(s.state.hyperlink(HyperlinkId(999)).is_none());
}

// ---- images ----

/// A Sixel image `width` x `height` px made of `bands` one-column bands.
fn image(width: u32, height: u32, bands: u32) -> Vec<u8> {
    let data = vec!["~"; bands as usize].join("-");
    format!("\x1bPq\"1;1;{width};{height}{data}\x1b\\").into_bytes()
}

fn goto(row: u16, col: u16) -> Vec<u8> {
    format!("\x1b[{};{}H", row + 1, col + 1).into_bytes()
}

#[test]
fn text_has_no_images() {
    let mut s = Session::settled();
    s.feed(b"text");
    s.update();
    assert!(s.state.placements().is_empty());
    assert!(
        s.state
            .rows()
            .iter()
            .flat_map(|row| &row.cells)
            .all(|cell| cell.graphic.is_none())
    );
    assert_eq!(s.state.graphic_offset(GraphicId(1), RowId(0), 0), None);
    assert!(s.state.placement(GraphicId(1)).is_none());
}

#[test]
fn placement_refills() {
    let mut s = Session::settled();
    s.feed(&goto(2, 3));
    s.feed(&image(25, 45, 8));
    assert_ne!(s.update(), SnapshotUpdate::Unchanged);
    let placement = *s.state.placements().first().expect("one placement");
    let top = s.state.rows()[2].id;
    assert_eq!(
        (placement.row, placement.col, placement.cols, placement.rows),
        (top, 3, 3, 3)
    );
    assert_eq!(placement.pixel_size, (25, 45));
    assert_eq!(s.state.placement(placement.id), Some(&placement));
    for (dy, dx) in (0..3).flat_map(|dy| (0..3).map(move |dx| (dy, dx))) {
        let cell = s.state.rows()[2 + usize::from(dy)].cells[3 + usize::from(dx)];
        assert_eq!(cell.graphic, Some(placement.id));
        let row = s.state.rows()[2 + usize::from(dy)].id;
        assert_eq!(
            s.state.graphic_offset(placement.id, row, 3 + dx),
            Some((dx, dy))
        );
    }
    assert_eq!(s.state.graphic_offset(placement.id, top, 2), None);
    assert_eq!(s.state.graphic_offset(placement.id, top, 6), None);
    assert_eq!(s.state.graphic_offset(placement.id, top - 1, 3), None);
    assert_eq!(s.state.graphic_offset(placement.id, top + 3, 3), None);
    assert_eq!(s.state.graphic_offset(GraphicId(999), top, 3), None);
    assert!(
        s.state.rows()[1]
            .cells
            .iter()
            .all(|cell| cell.graphic.is_none())
    );
}

#[test]
fn dead_anchor_skipped() {
    let mut s = Session::settled();
    s.feed(&image(10, 20, 1));
    s.feed(&goto(9, 0));
    s.feed(b"\n");
    s.update();
    assert_eq!(s.state.placements().len(), 1, "alive in the history");
    // Dropping the history kills the anchor; the engine releases the image at the next feed.
    s.term.set_scrollback_limit(0);
    s.update();
    assert!(s.state.placements().is_empty());
    assert_eq!(s.term.placements().len(), 1);
    s.feed(b"x");
    assert!(s.term.placements().is_empty());
    assert!(
        s.batch
            .iter()
            .any(|event| matches!(event, VtEvent::GraphicReleased(_)))
    );
}

#[test]
fn stale_graphic_has_no_placement() {
    let mut s = Session::settled();
    s.feed(b"\x1b[2;10r");
    s.feed(&goto(1, 0));
    s.feed(&image(10, 60, 1));
    s.update();
    let id = s.state.placements()[0].id;
    // Deleting the anchor row releases the image; the two rows below still name it.
    s.feed(&goto(1, 0));
    s.feed(b"\x1b[M");
    s.update();
    assert!(s.state.placements().is_empty());
    assert_eq!(s.state.rows()[1].cells[0].graphic, Some(id));
    assert!(s.state.placement(id).is_none());
    let row = s.state.rows()[1].id;
    assert_eq!(s.state.graphic_offset(id, row, 0), None);
}

#[test]
fn placement_only_change_is_partial() {
    let mut s = Session::settled();
    s.feed(&b"line\r\n".repeat(30));
    s.update();
    s.term.scroll_viewport(-15);
    s.update();
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);

    // A new image on the screen, out of view: no viewport row moved, the placements did.
    s.feed(&goto(5, 0));
    s.feed(&image(10, 20, 1));
    let before = s.state.rows_copied();
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert!(s.state.changed().is_empty());
    assert_eq!(s.state.rows_copied(), before);
    assert_eq!(s.state.placements().len(), 1);
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);

    // Evicting it changes the list again, still with no viewport row involved.
    s.feed(b"\x1bc");
    s.term.scroll_viewport(-5);
    s.update();
    assert!(s.state.placements().is_empty());
}

#[test]
fn suppressed_frame_holds_placement_meta() {
    let mut s = Session::settled();
    s.feed(&b"line\r\n".repeat(30));
    s.update();
    s.term.scroll_viewport(-15);
    s.update();

    s.feed(b"\x1b[?2026h");
    s.feed(&goto(5, 0));
    s.feed(&image(10, 20, 1));
    assert_eq!(s.update(), SnapshotUpdate::Unchanged, "the frame is held");
    assert_eq!(s.state.placements().len(), 1, "but the list is current");
    s.feed(b"\x1b[?2026l");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    assert!(s.state.changed().is_empty());
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
}

#[test]
fn placement_refresh_allocates_nothing() {
    let mut s = Session::settled();
    for index in 0..3 {
        s.feed(&goto(index * 3, 0));
        s.feed(&image(30, 60, 1));
    }
    // The lists swap each update, so both reach their high-water mark after two.
    s.update();
    s.update();
    let flat = s.state.buffer_capacities();
    assert!(flat[2] >= 3 && flat[3] >= 3);
    for cycle in 0..100 {
        s.feed(format!("\x1b[10;1Hcycle {cycle}").as_bytes());
        s.update();
        assert_eq!(s.state.buffer_capacities(), flat);
        assert_eq!(s.state.placements().len(), 3);
    }
}
// ---- palette ----

#[test]
fn palette_defaults() {
    let palette = Palette::new();
    assert_eq!(palette.indexed[1], rgb(0xcd, 0, 0));
    assert_eq!(palette.indexed[16], rgb(0, 0, 0));
    assert_eq!(palette.indexed[21], rgb(0, 0, 0xff));
    assert_eq!(palette.indexed[196], rgb(0xff, 0, 0));
    assert_eq!(palette.indexed[232], rgb(8, 8, 8));
    assert_eq!(palette.indexed[255], rgb(0xee, 0xee, 0xee));
    assert_eq!(palette.foreground, rgb(0xd8, 0xd8, 0xd8));
    assert_eq!(palette.background, rgb(0x18, 0x18, 0x18));
    assert_eq!(palette.cursor, rgb(0xd8, 0xd8, 0xd8));
    assert_eq!(Palette::default(), palette);
}

#[test]
fn palette_resolve_and_dim() {
    let mut palette = Palette::new();
    assert_eq!(palette.resolve(Color::Rgb(rgb(1, 2, 3))), rgb(1, 2, 3));
    assert_eq!(palette.resolve(Color::Palette(196)), rgb(0xff, 0, 0));
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::Red)),
        rgb(0xcd, 0, 0)
    );
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::BrightRed)),
        rgb(0xff, 0, 0)
    );
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::Foreground)),
        palette.foreground
    );
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::Background)),
        palette.background
    );
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::Cursor)),
        palette.cursor
    );

    palette.background = rgb(0, 0, 0);
    assert_eq!(palette.dim(rgb(205, 0, 0)), rgb(103, 0, 0));
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::DimRed)),
        rgb(103, 0, 0)
    );
    palette.background = rgb(40, 40, 60);
    assert_eq!(palette.dim(rgb(205, 0, 0)), rgb(123, 20, 30));

    // The optional slots fall back to the foreground and to a dimmed foreground.
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::BrightForeground)),
        palette.foreground
    );
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::DimForeground)),
        palette.dim(palette.foreground)
    );
    palette.bright_foreground = Some(rgb(9, 9, 9));
    palette.dim_foreground = Some(rgb(8, 8, 8));
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::BrightForeground)),
        rgb(9, 9, 9)
    );
    assert_eq!(
        palette.resolve(Color::Named(NamedColor::DimForeground)),
        rgb(8, 8, 8)
    );
}

/// A palette in which every colour is `shade`.
fn flat_palette(shade: u8) -> Palette {
    let mut palette = Palette::new();
    let color = rgb(shade, shade, shade);
    palette.indexed = [color; 256];
    palette.foreground = color;
    palette.background = color;
    palette.cursor = color;
    palette
}

#[test]
fn map_colors_is_idempotent() {
    let mut s = Session::new();
    s.feed(b"\x1b[31;48;5;21mred");
    s.update();
    let palette = Palette::new();
    s.state.map_colors(&palette);
    let once = s.state.rows().to_vec();
    s.state.map_colors(&palette);
    assert_eq!(s.state.rows(), once);
    let run = &s.state.rows()[0].runs[0];
    assert_eq!(run.style.fg, Color::Rgb(palette.indexed[1]));
    assert_eq!(run.style.bg, Color::Rgb(palette.indexed[21]));
    let blank = &s.state.rows()[1].runs[0];
    assert_eq!(blank.style.fg, Color::Rgb(palette.foreground));
    assert_eq!(blank.style.bg, Color::Rgb(palette.background));
}

#[test]
fn map_colors_resolves_the_underline_colour() {
    let mut s = Session::new();
    s.feed(b"\x1b[58;5;1mu");
    s.update();
    s.state.map_colors(&Palette::new());
    let run = &s.state.rows()[0].runs[0];
    assert_eq!(
        run.style.underline_color,
        Some(Color::Rgb(Palette::new().indexed[1]))
    );
}

#[test]
fn epoch_change_remaps_every_row() {
    let mut s = Session::new();
    s.fill_rows();
    s.update();
    s.state.map_colors(&Palette::new());
    s.feed(b"\x1b]4;1;rgb:ff/00/00\x07");
    assert_eq!(s.update(), SnapshotUpdate::Full);
    let themed = flat_palette(0x42);
    s.state.map_colors(&themed);
    for row in s.state.rows() {
        for run in &row.runs {
            assert_eq!(run.style.fg, Color::Rgb(rgb(0x42, 0x42, 0x42)));
            assert_eq!(run.style.bg, Color::Rgb(rgb(0x42, 0x42, 0x42)));
        }
    }
}

#[test]
fn same_epoch_remaps_changed_rows_only() {
    let mut s = Session::new();
    s.fill_rows();
    s.update();
    let (a, b) = (flat_palette(0x11), flat_palette(0x22));
    s.state.map_colors(&a);
    s.feed(b"\x1b[4;1Hnew");
    assert_eq!(s.update(), SnapshotUpdate::Partial { scrolled: 0 });
    s.state.map_colors(&b);
    for (index, row) in s.state.rows().iter().enumerate() {
        let expected = if index == 3 { 0x22 } else { 0x11 };
        assert_eq!(
            row.runs[0].style.fg,
            Color::Rgb(rgb(expected, expected, expected)),
            "row {index}"
        );
    }
}

#[test]
fn effective_palette_precedence() {
    let mut s = Session::new();
    let mut theme = Palette::new();
    theme.indexed[1] = rgb(0x11, 0x11, 0x11);
    theme.background = rgb(0x22, 0x22, 0x22);
    theme.bright_foreground = Some(rgb(0x33, 0x33, 0x33));
    s.term.set_theme_colors(&theme);
    assert_eq!(s.term.palette(), theme);

    s.feed(b"\x1b]4;1;rgb:ff/00/00\x07");
    assert_eq!(s.term.palette().indexed[1], rgb(0xff, 0, 0));
    assert_eq!(s.term.palette().indexed[2], theme.indexed[2]);
    s.feed(b"\x1b]104;1\x07");
    assert_eq!(s.term.palette().indexed[1], theme.indexed[1]);

    s.feed(b"\x1b]10;rgb:01/02/03\x07\x1b]11;rgb:04/05/06\x07\x1b]12;rgb:07/08/09\x07");
    let palette = s.term.palette();
    assert_eq!(palette.foreground, rgb(1, 2, 3));
    assert_eq!(palette.background, rgb(4, 5, 6));
    assert_eq!(palette.cursor, rgb(7, 8, 9));
    assert_eq!(palette.bright_foreground, theme.bright_foreground);
    s.feed(b"\x1b]110\x07\x1b]111\x07\x1b]112\x07");
    assert_eq!(s.term.palette(), theme);

    s.feed(b"\x1b]4;5;rgb:0a/0b/0c\x07\x1bc");
    assert_eq!(s.term.palette(), theme);
}

// ---- modes ----

#[test]
fn mode_snapshot_default_is_power_on() {
    let term = Terminal::new(Size { rows: 3, cols: 4 }, Config::default());
    assert_eq!(ModeSnapshot::default(), term.mode_snapshot());
    assert!(term.mode_snapshot().show_cursor);
    assert!(term.mode_snapshot().alternate_scroll);
}

#[test]
fn mode_snapshot_fields_follow_modes() {
    let after = |bytes: &[u8]| {
        let mut s = Session::new();
        s.feed(bytes);
        s.term.mode_snapshot()
    };
    let base = ModeSnapshot::default();
    assert!(after(b"\x1b[?1049h").alt_screen);
    assert!(after(b"\x1b[?1h").app_cursor);
    assert!(after(b"\x1b=").app_keypad);
    assert!(!after(b"\x1b=\x1b>").app_keypad);
    assert!(after(b"\x1b[?2004h").bracketed_paste);
    assert!(!after(b"\x1b[?25l").show_cursor);
    assert!(after(b"\x1b[4h").insert);
    assert!(!after(b"\x1b[?1007l").alternate_scroll);
    assert!(after(b"\x1b[?5h").reverse_video);
    assert_eq!(
        after(b"\x1b[>1u").keyboard_flags,
        KeyboardFlags::DISAMBIGUATE_ESC_CODES
    );
    assert_eq!(after(b"\x1b[>4;2m").modify_other_keys, 2);

    // Each sequence changes exactly one field.
    let mut only_insert = base;
    only_insert.insert = true;
    assert_eq!(after(b"\x1b[4h"), only_insert);
}

#[test]
fn mouse_protocol_derivation() {
    let protocol = |bytes: &[u8]| {
        let mut s = Session::new();
        s.feed(bytes);
        assert_eq!(s.term.mouse_reporting(), s.term.mode_snapshot().mouse);
        s.term.mouse_reporting()
    };
    let expect = |reporting, encoding| {
        Some(MouseProtocol {
            reporting,
            encoding,
        })
    };
    assert_eq!(protocol(b""), None);
    assert_eq!(
        protocol(b"\x1b[?1000h"),
        expect(MouseReporting::Normal, MouseEncoding::Default)
    );
    assert_eq!(
        protocol(b"\x1b[?1002h\x1b[?1006h"),
        expect(MouseReporting::ButtonEvent, MouseEncoding::Sgr)
    );
    assert_eq!(
        protocol(b"\x1b[?9h"),
        expect(MouseReporting::X10, MouseEncoding::Default)
    );
    assert_eq!(
        protocol(b"\x1b[?1003h\x1b[?1015h"),
        expect(MouseReporting::AnyEvent, MouseEncoding::Urxvt)
    );
    assert_eq!(
        protocol(b"\x1b[?1000h\x1b[?1005h"),
        expect(MouseReporting::Normal, MouseEncoding::Utf8)
    );
    assert_eq!(protocol(b"\x1b[?1005h"), None);
    assert_eq!(
        protocol(b"\x1b[?1003h\x1b[?1000l"),
        expect(MouseReporting::AnyEvent, MouseEncoding::Default)
    );
    assert_eq!(protocol(b"\x1b[?1003h\x1b[?1003l"), None);
}

// ---- allocation steadiness ----

#[test]
fn steady_state_grows_no_buffer() {
    type Capacities = (Vec<[usize; 3]>, [usize; 4]);
    let capacities = |s: &Session| -> Capacities {
        let rows = s
            .state
            .rows()
            .iter()
            .map(|row| {
                [
                    row.cells.capacity(),
                    row.runs.capacity(),
                    row.clusters.capacity(),
                ]
            })
            .collect();
        (rows, s.state.buffer_capacities())
    };
    let mut s = Session::sized(50, 200);
    s.update();
    let palette = Palette::new();
    let mut at_sixty = None;
    for cycle in 0..600usize {
        let row = cycle % 50;
        let color = 31 + cycle % 7;
        s.feed(
            format!(
                "\x1b[{};1H\x1b[{color}mcycle {cycle} \x1b[0m e\u{301} \x1b]8;;http://x/{row}\x07link\x1b]8;;\x07",
                row + 1
            )
            .as_bytes(),
        );
        assert_ne!(s.update(), SnapshotUpdate::Unchanged);
        s.state.map_colors(&palette);
        if cycle == 60 {
            at_sixty = Some(capacities(&s));
        }
    }
    assert_eq!(at_sixty, Some(capacities(&s)));
}

#[test]
fn idle_updates_copy_nothing() {
    let mut s = Session::settled();
    s.feed(b"some text");
    s.update();
    let copied = s.state.rows_copied();
    s.update();
    for _ in 0..100 {
        assert_eq!(s.update(), SnapshotUpdate::Unchanged);
    }
    assert_eq!(s.state.rows_copied(), copied);
}

#[test]
fn snapshot_state_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SnapshotState>();
    assert_send_sync::<SyncState>();
    assert_send_sync::<Palette>();
}
