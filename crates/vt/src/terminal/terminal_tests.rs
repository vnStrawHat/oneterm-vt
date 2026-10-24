//! Tests of the terminal: print path, controls, CSI and ESC, modes, resets, selection wrappers.

use std::time::Instant;

use super::*;
use crate::cell::{Attrs, Color, NamedColor, Rgb};
use crate::events::VtEvent;

/// A terminal with its batch, fed byte strings.
pub(super) struct Session {
    pub(super) term: Terminal,
    pub(super) batch: EventBatch,
}

impl Session {
    /// A default-config terminal of `cols` x `rows`.
    pub(super) fn new(cols: u16, rows: u16) -> Session {
        Session::sized(cols, rows, Config::default())
    }

    /// An 80x24 terminal with `config`.
    pub(super) fn with(config: Config) -> Session {
        Session::sized(80, 24, config)
    }

    pub(super) fn sized(cols: u16, rows: u16, config: Config) -> Session {
        Session {
            term: Terminal::new(Size { rows, cols }, config),
            batch: EventBatch::new(),
        }
    }

    pub(super) fn feed(&mut self, bytes: &[u8]) -> FeedStats {
        self.term.feed(bytes, &mut self.batch, Instant::now())
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

    /// The text of screen row `index`, blanks included.
    pub(super) fn row(&self, index: u16) -> String {
        self.term.row_text(self.term.screen().row_of_index(index))
    }

    /// The cursor as `(screen row index, column)`.
    pub(super) fn cursor(&self) -> (u16, u16) {
        let screen = self.term.screen();
        (screen.cursor_row_index(), screen.cursor().pos.col)
    }

    pub(super) fn style_at(&self, index: u16, col: u16) -> Style {
        let screen = self.term.screen();
        let cell = screen.row(screen.row_of_index(index)).cell(col);
        *self.term.interner().resolve_style(cell.style_id())
    }

    fn wrapped(&self, index: u16) -> bool {
        let screen = self.term.screen();
        screen.row(screen.row_of_index(index)).wrapped()
    }

    fn pos(&self, index: u16, col: u16) -> Pos {
        Pos {
            row: self.term.screen().row_of_index(index),
            col,
        }
    }

    /// The events of the last feed, without the trailing `Repaint`.
    fn events(&self) -> Vec<&VtEvent> {
        self.batch
            .iter()
            .filter(|event| **event != VtEvent::Repaint)
            .collect()
    }
}

#[test]
fn terminal_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Terminal>();
}

#[test]
fn ascii_prints_and_wraps() {
    let mut s = Session::new(4, 3);
    s.feed(b"abcdef");
    assert_eq!(s.row(0), "abcd");
    assert_eq!(s.row(1), "ef  ");
    assert!(s.wrapped(0));
    assert!(!s.wrapped(1));
    assert_eq!(s.cursor(), (1, 2));
}

#[test]
fn print_marks_the_feed_as_dispatched() {
    let mut s = Session::new(4, 3);
    s.feed(b"a");
    assert_eq!(s.batch.events(), [VtEvent::Repaint]);
}

#[test]
fn bell_pushes_an_event() {
    let mut s = Session::new(4, 3);
    s.feed(b"\x07");
    assert_eq!(s.events(), [&VtEvent::Bell]);
    assert_eq!(s.term.stats().unhandled_sequences, 0);
}

#[test]
fn backspace_at_column_zero_keeps_pending_wrap() {
    let mut s = Session::new(1, 3);
    s.feed(b"a");
    assert!(s.term.screen().cursor().pending_wrap);
    s.feed(b"\x08");
    assert_eq!(s.cursor(), (0, 0));
    assert!(s.term.screen().cursor().pending_wrap);
}

#[test]
fn backspace_moves_left_and_clears_pending_wrap() {
    let mut s = Session::new(4, 3);
    s.feed(b"abcd\x08");
    assert_eq!(s.cursor(), (0, 2));
    assert!(!s.term.screen().cursor().pending_wrap);
}

#[test]
fn tab_with_pending_wrap_wraps_instead() {
    let mut s = Session::new(4, 3);
    s.feed(b"abcd\t");
    assert_eq!(s.cursor(), (1, 0));
    assert!(s.wrapped(0));
}

#[test]
fn tab_moves_to_the_next_stop() {
    let mut s = Session::new(20, 3);
    s.feed(b"a\tb");
    assert_eq!(s.cursor(), (0, 9));
}

#[test]
fn vt_and_ff_act_as_line_feed() {
    let mut s = Session::new(10, 4);
    s.feed(b"a\x0bb\x0cc");
    assert_eq!(s.cursor(), (2, 3));
    assert_eq!(s.row(1).trim_end(), " b");
    assert_eq!(s.term.lines_produced(), 2);
}

#[test]
fn line_feed_never_implies_carriage_return() {
    let mut s = Session::new(10, 4);
    s.feed(b"ab\ncd");
    assert_eq!(s.row(1).trim_end(), "  cd");
}

#[test]
fn del_prints_nothing_and_is_not_counted() {
    let mut s = Session::new(10, 2);
    let stats = s.feed(b"a\x7fb");
    assert_eq!(s.row(0).trim_end(), "ab");
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn other_controls_and_c1_are_unhandled() {
    let mut s = Session::new(10, 2);
    assert_eq!(s.feed(b"\x01\x02").unhandled_sequences, 2);
    assert_eq!(s.feed(b"\x18").unhandled_sequences, 1);
    assert_eq!(s.feed(b"\x1a").unhandled_sequences, 1);
    assert_eq!(s.feed("\u{85}".as_bytes()).unhandled_sequences, 1);
}

#[test]
fn line_feeds_past_the_bottom_count_rows_scrolled() {
    let mut s = Session::new(10, 3);
    let stats = s.feed(b"1\n2\n3\n4\n5");
    assert_eq!(stats.rows_scrolled, 2);
    assert_eq!(s.term.screen().history_len(), 2);
    assert_eq!(s.term.lines_produced(), 4);
}

#[test]
fn wrap_at_the_bottom_scrolls_and_counts() {
    let mut s = Session::new(4, 2);
    let stats = s.feed(b"aaaabbbbcccc");
    assert_eq!(stats.rows_scrolled, 1);
    assert_eq!(s.term.lines_produced(), 0);
    assert_eq!(s.row(0), "bbbb");
    assert_eq!(s.row(1), "cccc");
}

#[test]
fn wrap_that_trims_history_pushes_rows_trimmed() {
    let config = Config {
        scrollback_limit: 1,
        ..Config::default()
    };
    let mut s = Session::sized(4, 2, config);
    let stats = s.feed(b"aaaabbbbccccdddd");
    assert_eq!(stats.rows_scrolled, 2);
    assert!(
        s.batch
            .iter()
            .any(|event| matches!(event, VtEvent::RowsTrimmed { .. }))
    );
}

#[test]
fn scroll_viewport_follows_the_documented_signs() {
    let mut s = Session::new(10, 3);
    s.feed(b"1\n2\n3\n4\n5\n6");
    let screen_top = s.term.screen().screen_top();
    s.term.scroll_viewport(-2);
    assert_eq!(s.term.viewport().top, screen_top - 2);
    s.term.scroll_viewport(1);
    assert_eq!(s.term.viewport().top, screen_top - 1);
    s.term.scroll_viewport(i32::MAX);
    assert_eq!(s.term.screen().scroll_offset(), 0);
    s.term.scroll_viewport(i32::MIN);
    assert_eq!(
        s.term.screen().scroll_offset(),
        s.term.screen().history_len()
    );
}

#[test]
fn feed_stats_are_per_call() {
    let mut s = Session::new(10, 2);
    assert_eq!(s.feed(b"\x01").unhandled_sequences, 1);
    let stats = s.feed(b"a");
    assert_eq!(stats.unhandled_sequences, 0);
    assert_eq!(stats.bytes, 1);
    assert_eq!(s.term.stats(), stats);
}

#[test]
fn empty_feed_clears_the_batch_and_stamps_nothing() {
    let mut s = Session::new(10, 2);
    s.feed(b"a");
    let stats = s.feed(b"");
    assert_eq!(stats, FeedStats::default());
    assert!(s.batch.is_empty());
}

#[test]
fn resize_reports_and_keeps_the_text() {
    let mut s = Session::new(10, 3);
    s.feed(b"hello");
    let outcome = s
        .term
        .resize(Size { rows: 3, cols: 6 }, ResizePolicy::BottomAnchor);
    assert!(outcome.reflowed);
    assert_eq!(s.term.size(), Size { rows: 3, cols: 6 });
    assert!(s.row(0).starts_with("hello"));
}

#[test]
fn resize_releases_a_dead_selection() {
    let config = Config {
        scrollback_limit: 0,
        ..Config::default()
    };
    let mut s = Session::sized(10, 4, config);
    s.term
        .selection_start(s.pos(3, 0), Side::Left, SelectionKind::Simple);
    s.term
        .resize(Size { rows: 2, cols: 10 }, ResizePolicy::BottomAnchor);
    assert!(s.term.state.selection.is_none());
}

#[test]
fn set_scrollback_limit_trims_and_rerings_without_changing_ids() {
    let config = Config {
        scrollback_limit: 100,
        ..Config::default()
    };
    let mut s = Session::sized(10, 3, config);
    for i in 0..20 {
        s.feed(format!("line{i}\r\n").as_bytes());
    }
    let newest = s.term.screen().newest();
    let kept: Vec<(RowId, String)> = (0..8)
        .map(|back| {
            let id = newest - back;
            (id, s.term.row_text(id))
        })
        .collect();
    s.term.select_all();
    assert!(s.term.has_selection());

    s.term.set_scrollback_limit(5);
    assert_eq!(s.term.config().scrollback_limit, 5);
    assert_eq!(s.term.screen().history_len(), 5);
    assert!(
        s.term.state.selection.is_none(),
        "an end died with the trim"
    );
    for (id, text) in &kept {
        assert_eq!(&s.term.row_text(*id), text);
    }

    let ring = s.term.screen().ring_len();
    s.term.set_scrollback_limit(5000);
    assert_ne!(s.term.screen().ring_len(), ring);
    for (id, text) in &kept {
        assert_eq!(&s.term.row_text(*id), text);
    }
    s.feed(b"more\r\n");
    assert_eq!(s.term.screen().history_len(), 6);
}

#[test]
fn hit_test_maps_pointer_positions() {
    let s = Session::new(10, 3);
    let top = s.term.viewport().top;
    let at = |row: f32, col: f32| s.term.hit_test(row, col);
    assert_eq!(
        at(1.2, 3.2),
        (
            Pos {
                row: top + 1,
                col: 3
            },
            Side::Left
        )
    );
    assert_eq!(
        at(1.2, 3.7),
        (
            Pos {
                row: top + 1,
                col: 3
            },
            Side::Right
        )
    );
    assert_eq!(at(-4.0, -1.0), (Pos { row: top, col: 0 }, Side::Left));
    assert_eq!(
        at(f32::NAN, f32::NAN),
        (Pos { row: top, col: 0 }, Side::Left)
    );
    assert_eq!(
        at(99.0, 9.2),
        (
            Pos {
                row: top + 2,
                col: 9
            },
            Side::Left
        )
    );
    assert_eq!(at(0.0, 10.0), (Pos { row: top, col: 9 }, Side::Right));
    assert_eq!(at(0.0, 1e9), (Pos { row: top, col: 9 }, Side::Right));
}

#[test]
fn selection_wrappers_follow_the_escape_set_of_the_config() {
    let mut s = Session::new(20, 3);
    s.feed(b"hello world");
    s.term
        .selection_start(s.pos(0, 6), Side::Left, SelectionKind::Semantic);
    assert!(s.term.has_selection());
    assert_eq!(s.term.selection_text().as_deref(), Some("world"));
    let range = s.term.selection_range().expect("a range");
    assert_eq!((range.start.col, range.end.col), (6, 10));

    let config = Config {
        semantic_escape_chars: String::new(),
        ..Config::default()
    };
    let mut open = Session::sized(20, 3, config);
    open.feed(b"hello world");
    open.term
        .selection_start(open.pos(0, 6), Side::Left, SelectionKind::Semantic);
    assert_eq!(open.term.selection_text().as_deref(), Some("hello world"));
}

#[test]
fn selection_update_clear_and_select_all() {
    let mut s = Session::new(20, 3);
    s.feed(b"hello world");
    s.term
        .selection_start(s.pos(0, 0), Side::Left, SelectionKind::Simple);
    assert!(!s.term.has_selection(), "a click selects nothing");
    s.term.selection_update(s.pos(0, 4), Side::Right);
    assert_eq!(s.term.selection_text().as_deref(), Some("hello"));
    s.term.selection_clear();
    s.term.selection_clear();
    assert_eq!(s.term.selection_text(), None);
    s.term.selection_update(s.pos(0, 4), Side::Right);
    assert!(!s.term.has_selection());
    s.term.select_all();
    assert_eq!(
        s.term.selection_text().as_deref(),
        Some(
            "hello world

"
        )
    );
}

#[test]
fn a_fresh_click_survives_output_and_a_dead_selection_is_released() {
    let config = Config {
        scrollback_limit: 0,
        ..Config::default()
    };
    let mut s = Session::sized(10, 3, config);
    s.term
        .selection_start(s.pos(0, 2), Side::Left, SelectionKind::Simple);
    s.feed(b"x");
    assert!(s.term.state.selection.is_some());
    // With no history, the next full-screen scroll drops the top row and the click with it.
    s.feed(b"\r\n\n\n");
    assert!(s.term.state.selection.is_none());
}

#[test]
fn accessors_report_power_on_state() {
    let s = Session::new(10, 3);
    assert_eq!(s.term.size(), Size { rows: 3, cols: 10 });
    assert_eq!(s.term.template(), Cell::EMPTY);
    assert_eq!(s.term.style(), Style::DEFAULT);
    assert_eq!(s.term.active_charset(), Charset::Ascii);
    assert_eq!(s.term.lines_produced(), 0);
    assert_eq!(s.term.title(), None);
    assert_eq!(s.term.title_depth(), 0);
    assert!(format!("{:?}", s.term).contains("Terminal"));
}

#[test]
fn session_helpers_read_back_replies_and_styles() {
    let mut s = Session::with(Config::default());
    s.feed(b"a\x1b[c");
    assert_eq!(s.replies(), "\x1b[?62;4;22c");
    assert_eq!(s.style_at(0, 0), Style::DEFAULT);
    assert_eq!(s.term.size(), Size { rows: 24, cols: 80 });
}

// ---- Charsets, shifts, REP ----

#[test]
fn line_drawing_through_g1_and_shift_out() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b)0\x0eqqq\x0fqqq");
    assert_eq!(s.row(0).trim_end(), "───qqq");
}

#[test]
fn line_drawing_through_g0_and_back() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b(0lqk\x1b(Blqk");
    assert_eq!(s.row(0).trim_end(), "┌─┐lqk");
}

#[test]
fn every_line_drawing_mapping() {
    let mut s = Session::new(40, 2);
    s.feed(b"\x1b(0_`afgjklmnopqrstuvwxyz{|}~");
    assert_eq!(s.row(0).trim_end(), " ◆▒°±┘┐┌└┼⎺⎻─⎼⎽├┤┴┬│≤≥π≠£·");
}

#[test]
fn decsc_decrc_restore_designations_and_shifts() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b)0\x0e\x1b7\x1b)B\x0f");
    assert_eq!(s.term.active_charset(), Charset::Ascii);
    s.feed(b"\x1b8q");
    assert_eq!(
        s.term.active_charset(),
        Charset::SpecialCharacterAndLineDrawing
    );
    assert_eq!(s.row(0).trim_end(), "─");
}

#[test]
fn single_shifts_apply_to_one_character_and_survive_sgr() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b*0\x1bNqq");
    assert_eq!(s.row(0).trim_end(), "─q");
    s.feed(b"\r\x1b+0\x1bO\x1b[mqq");
    assert_eq!(s.row(0).trim_end(), "─q");
}

#[test]
fn locking_shifts_persist() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b*0\x1bnqq");
    assert_eq!(s.row(0).trim_end(), "──");
    s.feed(b"\r\x0f\x1b+0\x1bo\x1b[mqq");
    assert_eq!(s.row(0).trim_end(), "──");
    assert_eq!(
        s.term.active_charset(),
        Charset::SpecialCharacterAndLineDrawing
    );
}

#[test]
fn unknown_designations_are_unhandled() {
    let mut s = Session::new(10, 2);
    assert_eq!(s.feed(b"\x1b(A").unhandled_sequences, 1);
    assert_eq!(s.feed(b"\x1b-B").unhandled_sequences, 1);
}

#[test]
fn rep_repeats_the_last_scalar_through_the_wrap() {
    let mut s = Session::new(4, 3);
    s.feed(b"ab\x1b[4b");
    assert_eq!(s.row(0), "abbb");
    assert_eq!(s.row(1), "bb  ");
}

#[test]
fn rep_source_survives_sgr_and_defaults_to_one() {
    let mut s = Session::new(10, 2);
    s.feed(b"ab\x1b[32m\x1b[b\x1b[2b");
    assert_eq!(s.row(0).trim_end(), "abbbb");
}

#[test]
fn rep_before_any_print_does_nothing() {
    let mut s = Session::new(10, 2);
    let stats = s.feed(b"\x1b[3b");
    assert_eq!(s.row(0).trim_end(), "");
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn rep_uses_the_scalar_before_charset_mapping() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b(0q\x1b[2b");
    assert_eq!(s.row(0).trim_end(), "───");
}

// ---- ?2027 ----

const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";

#[test]
fn cluster_mode_joins_a_family_split_across_feeds() {
    let mut s = Session::new(10, 2);
    s.feed("\x1b[?2027h\u{1F468}\u{200D}\u{1F469}".as_bytes());
    assert_eq!(s.cursor(), (0, 2));
    s.feed("\u{200D}\u{1F467}x".as_bytes());
    assert_eq!(s.row(0).trim_end(), format!("{FAMILY}x"));
    assert_eq!(s.cursor(), (0, 3));
}

#[test]
fn cluster_mode_breaks_at_a_control() {
    let mut s = Session::new(10, 2);
    s.feed("\x1b[?2027h\u{1F468}\u{200D}".as_bytes());
    s.feed(b"\x07");
    s.feed("\u{1F469}".as_bytes());
    assert_eq!(s.cursor(), (0, 4));
}

#[test]
fn cluster_mode_places_whole_clusters_in_one_feed() {
    let mut s = Session::new(10, 2);
    s.feed(format!("\x1b[?2027h{FAMILY}e\u{301}x").as_bytes());
    assert_eq!(s.cursor(), (0, 4));
    assert_eq!(s.row(0).trim_end(), format!("{FAMILY}e\u{301}x"));
}

#[test]
fn a_cluster_over_32_scalars_is_not_carried() {
    let mut s = Session::new(10, 2);
    let stream = format!("\x1b[?2027he{}", "\u{301}".repeat(39));
    let stats = s.feed(stream.as_bytes());
    assert_eq!(stats.dropped_cluster_carries, 1);
    assert!(s.term.state.carry.is_none());
    assert_eq!(s.feed("\u{301}".as_bytes()).dropped_cluster_carries, 0);
}

#[test]
fn a_cluster_of_exactly_32_scalars_is_carried() {
    let mut s = Session::new(10, 2);
    let stream = format!("\x1b[?2027he{}", "\u{301}".repeat(31));
    let stats = s.feed(stream.as_bytes());
    assert_eq!(stats.dropped_cluster_carries, 0);
    assert!(s.term.state.carry.is_some());
}

#[test]
fn a_lone_variation_selector_has_width_zero() {
    let mut s = Session::new(10, 2);
    s.feed("\x1b[?2027h\u{FE0F}".as_bytes());
    assert_eq!(s.cursor(), (0, 0));
    assert!(s.term.state.carry.is_none());
}

#[test]
fn a_carried_cluster_grows_in_place_under_insert_mode() {
    let mut whole = Session::new(10, 3);
    whole.feed("\x1b[?2027h\x1b[4he\u{301}".as_bytes());
    let mut split = Session::new(10, 3);
    split.feed(b"\x1b[?2027h\x1b[4he");
    split.feed("\u{301}".as_bytes());
    assert_eq!(whole.row(0).trim_end(), "e\u{301}");
    assert_eq!(split.row(0).trim_end(), "e\u{301}");
    assert_eq!(split.cursor(), whole.cursor());
}

#[test]
fn rep_repeats_the_received_scalar_not_the_charset_glyph() {
    for cluster_mode in ["", "\x1b[?2027h"] {
        let mut s = Session::new(10, 2);
        s.feed(format!("{cluster_mode}\x1b(0q\x1b(B\x1b[3b").as_bytes());
        assert_eq!(
            s.row(0).trim_end(),
            "\u{2500}qqq",
            "cluster mode {cluster_mode:?}"
        );
    }
}

#[test]
fn resize_and_reset_drop_the_cluster_carry() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b[?2027he");
    assert!(s.term.state.carry.is_some());
    s.term
        .resize(Size { rows: 2, cols: 12 }, ResizePolicy::BottomAnchor);
    assert!(s.term.state.carry.is_none());
    s.feed(b"e");
    assert!(s.term.state.carry.is_some());
    s.feed(b"\x1b[?2027l");
    assert!(s.term.state.carry.is_none());
    s.feed(b"\x1b[?2027he\x1bc");
    assert!(s.term.state.carry.is_none());
}

// ---- C0 with modes ----

#[test]
fn reverse_wrap_crosses_only_into_a_wrapped_row() {
    let mut s = Session::new(4, 4);
    s.feed(b"\x1b[?45habcdef\r\x08");
    assert_eq!(s.cursor(), (0, 3));
    s.feed(b"\x1b[3;1H\x08");
    assert_eq!(s.cursor(), (2, 0), "row 1 did not wrap into row 2");
}

#[test]
fn reverse_wrap_never_crosses_above_the_region_top() {
    let mut s = Session::new(4, 4);
    s.feed(b"abcdef\x1b[2;4r\x1b[2;1H\x1b[?45h\x08");
    assert_eq!(s.cursor(), (1, 0));
}

#[test]
fn backspace_without_reverse_wrap_stays_at_column_zero() {
    let mut s = Session::new(4, 4);
    s.feed(b"abcdef\r\x08");
    assert_eq!(s.cursor(), (1, 0));
}

// ---- Cursor motion ----

/// Puts the cursor at index 5, column 5 of a 20x10 screen, feeds `sequence`, returns the cursor.
fn moved(sequence: &str) -> (u16, u16) {
    let mut s = Session::new(20, 10);
    s.feed(b"\x1b[6;6H");
    s.feed(sequence.as_bytes());
    s.cursor()
}

#[test]
fn relative_cursor_moves_with_and_without_a_count() {
    for (sequence, expected) in [
        ("\x1b[A", (4, 5)),
        ("\x1b[0A", (4, 5)),
        ("\x1b[3A", (2, 5)),
        ("\x1b[99A", (0, 5)),
        ("\x1b[B", (6, 5)),
        ("\x1b[0B", (6, 5)),
        ("\x1b[3B", (8, 5)),
        ("\x1b[99B", (9, 5)),
        ("\x1b[e", (6, 5)),
        ("\x1b[2e", (7, 5)),
        ("\x1b[C", (5, 6)),
        ("\x1b[0C", (5, 6)),
        ("\x1b[3C", (5, 8)),
        ("\x1b[99C", (5, 19)),
        ("\x1b[a", (5, 6)),
        ("\x1b[D", (5, 4)),
        ("\x1b[0D", (5, 4)),
        ("\x1b[3D", (5, 2)),
        ("\x1b[99D", (5, 0)),
        ("\x1b[E", (6, 0)),
        ("\x1b[3E", (8, 0)),
        ("\x1b[F", (4, 0)),
        ("\x1b[3F", (2, 0)),
    ] {
        assert_eq!(moved(sequence), expected, "{sequence:?}");
    }
}

#[test]
fn absolute_cursor_moves_with_and_without_a_parameter() {
    for (sequence, expected) in [
        ("\x1b[G", (5, 0)),
        ("\x1b[0G", (5, 0)),
        ("\x1b[10G", (5, 9)),
        ("\x1b[99G", (5, 19)),
        ("\x1b[`", (5, 0)),
        ("\x1b[10`", (5, 9)),
        ("\x1b[d", (0, 5)),
        ("\x1b[3d", (2, 5)),
        ("\x1b[99d", (9, 5)),
        ("\x1b[H", (0, 0)),
        ("\x1b[3;4H", (2, 3)),
        ("\x1b[;4H", (0, 3)),
        ("\x1b[3H", (2, 0)),
        ("\x1b[0;0H", (0, 0)),
        ("\x1b[99;99H", (9, 19)),
        ("\x1b[f", (0, 0)),
        ("\x1b[3;4f", (2, 3)),
    ] {
        assert_eq!(moved(sequence), expected, "{sequence:?}");
    }
}

#[test]
fn tab_motion_with_and_without_a_count() {
    for (sequence, expected) in [
        ("\x1b[I", (5, 8)),
        ("\x1b[0I", (5, 8)),
        ("\x1b[2I", (5, 16)),
        ("\x1b[9I", (5, 19)),
        ("\x1b[Z", (5, 0)),
        ("\x1b[0Z", (5, 0)),
        ("\x1b[3Z", (5, 0)),
    ] {
        assert_eq!(moved(sequence), expected, "{sequence:?}");
    }
    let mut s = Session::new(20, 4);
    s.feed(b"\x1b[1;15H\x1b[Z");
    assert_eq!(s.cursor(), (0, 8));
    s.feed(b"\x1b[2Z");
    assert_eq!(s.cursor(), (0, 0));
}

#[test]
fn tab_motion_keeps_pending_wrap() {
    let mut s = Session::new(4, 3);
    s.feed(b"abcd\x1b[I");
    assert!(s.term.screen().cursor().pending_wrap);
    assert_eq!(s.cursor(), (0, 3));
    s.feed(b"\x1b[Z");
    assert!(s.term.screen().cursor().pending_wrap);
}

#[test]
fn vertical_moves_stop_at_the_region_margins() {
    let mut s = Session::new(20, 10);
    s.feed(b"\x1b[3;7r\x1b[5;1H");
    s.feed(b"\x1b[99A");
    assert_eq!(s.cursor(), (2, 0), "inside the region, stops at its top");
    s.feed(b"\x1b[99B");
    assert_eq!(s.cursor(), (6, 0), "inside the region, stops at its bottom");
    s.feed(b"\x1b[9;1H\x1b[99B");
    assert_eq!(
        s.cursor(),
        (9, 0),
        "below the region, stops at the screen edge"
    );
    s.feed(b"\x1b[9;1H\x1b[99A");
    assert_eq!(s.cursor(), (2, 0), "below the region, climbs to its top");
    s.feed(b"\x1b[1;1H\x1b[99B");
    assert_eq!(s.cursor(), (6, 0), "above the region, stops at its bottom");
    s.feed(b"\x1b[2;1H\x1b[99A");
    assert_eq!(
        s.cursor(),
        (0, 0),
        "above the region, stops at the screen top"
    );
}

#[test]
fn origin_mode_only_changes_absolute_row_moves() {
    let mut s = Session::new(20, 10);
    s.feed(b"\x1b[3;7r\x1b[?6h");
    assert_eq!(s.cursor(), (2, 0));
    s.feed(b"\x1b[5A");
    assert_eq!(s.cursor(), (2, 0), "CUU never moves up past the region");
    s.feed(b"\x1b[B");
    assert_eq!(s.cursor(), (3, 0));
    s.feed(b"\x1b[4G");
    assert_eq!(s.cursor(), (3, 3), "CHA keeps the row");
    s.feed(b"\x1b[1;1H");
    assert_eq!(s.cursor(), (2, 0), "CUP is region-relative");
    s.feed(b"\x1b[2d");
    assert_eq!(s.cursor(), (3, 0), "VPA is region-relative");
    s.feed(b"\x1b[99;5H");
    assert_eq!(s.cursor(), (6, 4), "CUP clamps to the region bottom");
}

#[test]
fn cursor_moves_clear_pending_wrap() {
    let mut s = Session::new(4, 3);
    s.feed(b"abcd\x1b[A");
    assert!(!s.term.screen().cursor().pending_wrap);
}

// ---- Region ----

#[test]
fn decstbm_sets_the_region_and_homes() {
    let mut s = Session::new(20, 24);
    s.feed(b"\x1b[5;5H\x1b[3;10r");
    assert_eq!(s.term.screen().region().top, 2);
    assert_eq!(s.term.screen().region().bottom, 10);
    assert_eq!(s.cursor(), (0, 0));
    s.feed(b"\x1b[r");
    assert_eq!(s.term.screen().region().bottom, 24);
    s.feed(b"\x1b[4r");
    assert_eq!(s.term.screen().region().top, 3);
    assert_eq!(s.term.screen().region().bottom, 24);
}

#[test]
fn invalid_decstbm_changes_nothing_and_is_not_counted() {
    let mut s = Session::new(20, 24);
    s.feed(b"\x1b[3;10r\x1b[5;5H");
    for sequence in [&b"\x1b[5;3r"[..], b"\x1b[3;3r", b"\x1b[30;40r"] {
        let stats = s.feed(sequence);
        assert_eq!(stats.unhandled_sequences, 0, "{sequence:?}");
        assert_eq!(s.term.screen().region().top, 2, "{sequence:?}");
        assert_eq!(s.term.screen().region().bottom, 10, "{sequence:?}");
        assert_eq!(s.cursor(), (4, 4), "{sequence:?}");
    }
}

// ---- Editing ----

fn edited(sequence: &str) -> String {
    let mut s = Session::new(10, 3);
    s.feed(b"abcdef\x1b[1;3H");
    s.feed(sequence.as_bytes());
    s.row(0).trim_end().to_owned()
}

#[test]
fn character_edits_with_and_without_counts() {
    for (sequence, expected) in [
        ("\x1b[@", "ab cdef"),
        ("\x1b[0@", "ab cdef"),
        ("\x1b[2@", "ab  cdef"),
        ("\x1b[P", "abdef"),
        ("\x1b[2P", "abef"),
        ("\x1b[X", "ab def"),
        ("\x1b[3X", "ab   f"),
        ("\x1b[K", "ab"),
        ("\x1b[0K", "ab"),
        ("\x1b[1K", "   def"),
        ("\x1b[2K", ""),
    ] {
        assert_eq!(edited(sequence), expected, "{sequence:?}");
    }
    let mut s = Session::new(10, 3);
    assert_eq!(s.feed(b"\x1b[3K").unhandled_sequences, 1);
}

#[test]
fn line_edits_move_rows() {
    let mut s = Session::new(4, 5);
    s.feed(b"1\r\n2\r\n3\r\n4\r\n5\x1b[2;1H\x1b[L");
    let rows: Vec<String> = (0..5).map(|i| s.row(i).trim_end().to_owned()).collect();
    assert_eq!(rows, ["1", "", "2", "3", "4"]);
    s.feed(b"\x1b[2M");
    let rows: Vec<String> = (0..5).map(|i| s.row(i).trim_end().to_owned()).collect();
    assert_eq!(rows, ["1", "3", "4", "", ""]);
}

// ---- Tab stops ----

#[test]
fn tab_stop_management() {
    let mut s = Session::new(30, 2);
    s.feed(b"\x1b[1;4H\x1bH\r\t");
    assert_eq!(s.cursor(), (0, 3));
    s.feed(b"\x1b[1;9H\x1b[g\r\t\t");
    assert_eq!(s.cursor(), (0, 16));
    s.feed(b"\x1b[3g\r\t");
    assert_eq!(s.cursor(), (0, 29));
    s.feed(b"\x1b[?5W\r\t");
    assert_eq!(s.cursor(), (0, 8));
    assert_eq!(s.feed(b"\x1b[2g").unhandled_sequences, 1);
    assert_eq!(s.feed(b"\x1b[?4W").unhandled_sequences, 1);
}

// ---- ESC ----

#[test]
fn ind_nel_and_ri() {
    let mut s = Session::new(10, 3);
    s.feed(b"ab\x1bDc\x1bEd");
    assert_eq!(s.row(1).trim_end(), "  c");
    assert_eq!(s.row(2).trim_end(), "d");
    assert_eq!(s.term.lines_produced(), 2);
    s.feed(b"\x1b[1;1H\x1bM");
    assert!(
        s.batch
            .iter()
            .any(|event| matches!(event, VtEvent::RowsScrolled(_)))
    );
    assert_eq!(s.row(0).trim_end(), "");
    assert_eq!(s.row(1).trim_end(), "ab");
}

#[test]
fn decaln_fills_with_e_and_keeps_cursor_and_region() {
    let mut s = Session::new(6, 5);
    s.feed(b"\x1b[1m\x1b[2;4r\x1b#8");
    for index in 0..5 {
        assert_eq!(s.row(index), "EEEEEE");
    }
    assert_eq!(s.cursor(), (0, 0), "DECSTBM homed before DECALN");
    assert_eq!(s.term.screen().region().top, 1);
    assert_eq!(s.style_at(0, 0), Style::DEFAULT, "the pen is ignored");
}

#[test]
fn application_keypad_follows_esc_equals_and_greater() {
    let mut s = Session::new(6, 2);
    s.feed(b"\x1b=");
    assert!(s.term.mode(Mode::AppKeypad));
    s.feed(b"\x1b>");
    assert!(!s.term.mode(Mode::AppKeypad));
}

#[test]
fn string_terminator_is_silent_and_unknown_escapes_count() {
    let mut s = Session::new(6, 2);
    assert_eq!(s.feed(b"\x1b\\").unhandled_sequences, 0);
    assert_eq!(s.feed(b"\x1b6").unhandled_sequences, 1);
}

#[test]
fn unknown_and_unimplemented_csi_are_counted() {
    let mut s = Session::new(10, 2);
    for sequence in [
        &b"\x1b[5J"[..],
        b"\x1b[9 q",
        b"\x1b[1c",
        b"\x1b[\"q",
        b"\x1b[?J",
        b"\x1b[?K",
        b"\x1b[99z",
        b"\x1b[?9999z",
    ] {
        assert_eq!(s.feed(sequence).unhandled_sequences, 1, "{sequence:?}");
    }
    let many = format!("\x1b[{}A", vec!["1"; 40].join(";"));
    assert_eq!(s.feed(many.as_bytes()).unhandled_sequences, 1);
}

#[test]
fn set_mode_counts_one_unhandled_per_unknown_number() {
    let mut s = Session::new(10, 2);
    assert_eq!(s.feed(b"\x1b[?1;9999;8888h").unhandled_sequences, 2);
    assert!(s.term.mode(Mode::AppCursor));
    assert_eq!(s.feed(b"\x1b[7h").unhandled_sequences, 1);
}

#[test]
fn apc_and_unknown_dcs_are_unhandled() {
    let mut s = Session::new(10, 2);
    assert_eq!(s.feed(b"\x1b_payload\x1b\\").unhandled_sequences, 1);
    assert_eq!(s.feed(b"\x1b^payload\x1b\\").unhandled_sequences, 1);
    assert_eq!(s.feed(b"\x1bXpayload\x1b\\").unhandled_sequences, 1);
    assert_eq!(s.feed(b"\x1bP1xdata\x1b\\").unhandled_sequences, 1);
    assert_eq!(s.feed(b"\x1bPqdata\x1b\\").unhandled_sequences, 1);
}

#[test]
fn a_cancelled_dcs_counts_as_aborted() {
    let mut s = Session::new(10, 2);
    let stats = s.feed(b"\x1bP1xdata\x18");
    assert_eq!(stats.aborted_dcs, 1);
    let stats = s.feed(b"\x1bP$qm\x18");
    assert_eq!(stats.aborted_dcs, 1);
    assert_eq!(s.replies(), "");
}

// ---- SGR ----

fn pen_after(sequence: &str) -> Style {
    let mut s = Session::new(10, 2);
    s.feed(sequence.as_bytes());
    s.term.style()
}

fn with_attrs(attrs: Attrs) -> Style {
    Style {
        attrs,
        ..Style::DEFAULT
    }
}

fn fg(color: Color) -> Style {
    Style {
        fg: color,
        ..Style::DEFAULT
    }
}

fn bg(color: Color) -> Style {
    Style {
        bg: color,
        ..Style::DEFAULT
    }
}

#[test]
fn sgr_attribute_table() {
    for (sequence, attrs) in [
        ("\x1b[1m", Attrs::BOLD),
        ("\x1b[2m", Attrs::DIM),
        ("\x1b[3m", Attrs::ITALIC),
        ("\x1b[4m", Attrs::UNDERLINE),
        ("\x1b[4:1m", Attrs::UNDERLINE),
        ("\x1b[4:6m", Attrs::UNDERLINE),
        ("\x1b[4:2m", Attrs::DOUBLE_UNDERLINE),
        ("\x1b[21m", Attrs::DOUBLE_UNDERLINE),
        ("\x1b[4:3m", Attrs::UNDERCURL),
        ("\x1b[4:4m", Attrs::DOTTED_UNDERLINE),
        ("\x1b[4:5m", Attrs::DASHED_UNDERLINE),
        ("\x1b[5m", Attrs::BLINK_SLOW),
        ("\x1b[6m", Attrs::BLINK_FAST),
        ("\x1b[7m", Attrs::INVERSE),
        ("\x1b[8m", Attrs::HIDDEN),
        ("\x1b[9m", Attrs::STRIKEOUT),
        ("\x1b[53m", Attrs::OVERLINE),
        ("\x1b[1;2m", Attrs::BOLD | Attrs::DIM),
        ("\x1b[4;21m", Attrs::DOUBLE_UNDERLINE),
        ("\x1b[21;4m", Attrs::UNDERLINE),
    ] {
        assert_eq!(pen_after(sequence), with_attrs(attrs), "{sequence:?}");
    }
}

#[test]
fn sgr_removals_and_resets() {
    for (sequence, expected) in [
        ("\x1b[1;2;22m", Attrs::empty()),
        ("\x1b[3;23m", Attrs::empty()),
        ("\x1b[4;24m", Attrs::empty()),
        ("\x1b[4:3;4:0m", Attrs::empty()),
        ("\x1b[5;6;25m", Attrs::empty()),
        ("\x1b[7;27m", Attrs::empty()),
        ("\x1b[8;28m", Attrs::empty()),
        ("\x1b[9;29m", Attrs::empty()),
        ("\x1b[53;55m", Attrs::empty()),
        ("\x1b[1;3;22m", Attrs::ITALIC),
    ] {
        assert_eq!(pen_after(sequence), with_attrs(expected), "{sequence:?}");
    }
    for sequence in ["\x1b[1;31m\x1b[0m", "\x1b[1;31m\x1b[m", "\x1b[1;31;0m"] {
        assert_eq!(pen_after(sequence), Style::DEFAULT, "{sequence:?}");
    }
}

#[test]
fn sgr_named_colours() {
    const NAMES: [NamedColor; 16] = [
        NamedColor::Black,
        NamedColor::Red,
        NamedColor::Green,
        NamedColor::Yellow,
        NamedColor::Blue,
        NamedColor::Magenta,
        NamedColor::Cyan,
        NamedColor::White,
        NamedColor::BrightBlack,
        NamedColor::BrightRed,
        NamedColor::BrightGreen,
        NamedColor::BrightYellow,
        NamedColor::BrightBlue,
        NamedColor::BrightMagenta,
        NamedColor::BrightCyan,
        NamedColor::BrightWhite,
    ];
    for index in 0..8u16 {
        let name = Color::Named(NAMES[usize::from(index)]);
        let bright = Color::Named(NAMES[usize::from(index) + 8]);
        assert_eq!(pen_after(&format!("\x1b[{}m", 30 + index)), fg(name));
        assert_eq!(pen_after(&format!("\x1b[{}m", 90 + index)), fg(bright));
        assert_eq!(pen_after(&format!("\x1b[{}m", 40 + index)), bg(name));
        assert_eq!(pen_after(&format!("\x1b[{}m", 100 + index)), bg(bright));
    }
    assert_eq!(pen_after("\x1b[31;39m"), Style::DEFAULT);
    assert_eq!(pen_after("\x1b[41;49m"), Style::DEFAULT);
    assert_eq!(pen_after("\x1b[58;5;1;59m"), Style::DEFAULT);
}

#[test]
fn sgr_extended_colours() {
    let rgb = |r, g, b| Color::Rgb(Rgb { r, g, b });
    let underline = |color| Style {
        underline_color: Some(color),
        ..Style::DEFAULT
    };
    for (sequence, expected) in [
        ("\x1b[38;5;200m", fg(Color::Palette(200))),
        ("\x1b[38;2;1;2;3m", fg(rgb(1, 2, 3))),
        ("\x1b[48;5;7m", bg(Color::Palette(7))),
        ("\x1b[48;2;4;5;6m", bg(rgb(4, 5, 6))),
        ("\x1b[58;5;9m", underline(Color::Palette(9))),
        ("\x1b[58;2;7;8;9m", underline(rgb(7, 8, 9))),
        ("\x1b[38:5:200m", fg(Color::Palette(200))),
        ("\x1b[38:2:1:2:3m", fg(rgb(1, 2, 3))),
        ("\x1b[38:2::1:2:3m", fg(rgb(1, 2, 3))),
        ("\x1b[38:2:7:1:2:3m", fg(rgb(1, 2, 3))),
        ("\x1b[48:2::4:5:6m", bg(rgb(4, 5, 6))),
        ("\x1b[58:5:9m", underline(Color::Palette(9))),
        (
            "\x1b[38;5;1;2m",
            Style {
                fg: Color::Palette(1),
                attrs: Attrs::DIM,
                ..Style::DEFAULT
            },
        ),
    ] {
        assert_eq!(pen_after(sequence), expected, "{sequence:?}");
    }
}

#[test]
fn sgr_bad_extended_colours_are_dropped() {
    for sequence in [
        "\x1b[38;5m",
        "\x1b[38;5;300m",
        "\x1b[38;2;1;2m",
        "\x1b[38;9m",
        "\x1b[38m",
        "\x1b[38:5m",
        "\x1b[38:2:1:2m",
        "\x1b[38:7:1m",
    ] {
        assert_eq!(pen_after(sequence), Style::DEFAULT, "{sequence:?}");
    }
    // The values the bad colour read are consumed; the rest still applies.
    assert_eq!(
        pen_after("\x1b[38;2;300;1;2m"),
        with_attrs(Attrs::BOLD | Attrs::DIM)
    );
}

#[test]
fn sgr_unknown_groups_are_skipped() {
    assert_eq!(
        pen_after("\x1b[1;99;3;1000m"),
        with_attrs(Attrs::BOLD | Attrs::ITALIC)
    );
}

#[test]
fn sgr_colours_reach_printed_cells_and_erases() {
    let mut s = Session::new(6, 2);
    s.feed(b"\x1b[41mab\x1b[K");
    assert_eq!(s.style_at(0, 0), bg(Color::Named(NamedColor::Red)));
    assert_eq!(s.style_at(0, 5), bg(Color::Named(NamedColor::Red)));
    s.feed(b"\x1b[0mc");
    assert_eq!(s.style_at(0, 2), Style::DEFAULT);
}

#[test]
fn sgr_leaves_the_zone_of_the_pen() {
    let mut s = Session::new(6, 2);
    s.feed(b"\x1b]133;A\x07\x1b[1m");
    assert_eq!(s.term.template().semantic(), crate::cell::Semantic::Prompt);
}

// ---- Scroll events ----

#[test]
fn erase_display_2_scrolls_into_history_after_screen_cleared() {
    let mut s = Session::new(10, 3);
    s.feed(b"a\r\nb\r\nc");
    let stats = s.feed(b"\x1b[2J");
    assert_eq!(s.events(), [&VtEvent::ScreenCleared]);
    assert_eq!(s.term.screen().history_len(), 3);
    assert_eq!(stats.rows_scrolled, 3);
    assert_eq!(s.row(0).trim_end(), "");
    assert_eq!(s.term.lines_produced(), 2);
}

#[test]
fn erase_display_3_reports_even_without_history() {
    let mut s = Session::new(10, 3);
    s.feed(b"\x1b[3J");
    assert_eq!(s.events(), [&VtEvent::ScreenCleared]);
    s.feed(b"1\n2\n3\n4\n5\x1b[3J");
    assert_eq!(s.term.screen().history_len(), 0);
    assert!(matches!(
        s.events().as_slice(),
        [VtEvent::ScreenCleared, VtEvent::RowsTrimmed { .. }]
    ));
}

#[test]
fn erase_display_0_and_1_push_no_events() {
    let mut s = Session::new(10, 3);
    s.feed(b"abc\r\ndef\x1b[0J\x1b[1J");
    assert!(s.events().is_empty());
    assert_eq!(s.feed(b"\x1b[5J").unhandled_sequences, 1);
}

#[test]
fn region_scrolls_and_line_edits_push_rows_scrolled() {
    for sequence in [&b"\x1b[S"[..], b"\x1b[T", b"\x1b[L", b"\x1b[M"] {
        let mut s = Session::new(10, 5);
        s.feed(b"\x1b[2;3r\x1b[2;1H");
        s.feed(sequence);
        assert!(
            s.batch
                .iter()
                .any(|event| matches!(event, VtEvent::RowsScrolled(_))),
            "{sequence:?}"
        );
    }
}

#[test]
fn a_wrap_at_the_bottom_of_a_partial_region_pushes_rows_scrolled() {
    let mut s = Session::new(4, 3);
    s.feed(b"\x1b[1;2raaaabbbbcc");
    assert!(
        s.batch
            .iter()
            .any(|event| matches!(event, VtEvent::RowsScrolled(_)))
    );
    assert_eq!(s.row(0), "bbbb");
    assert_eq!(s.row(1), "cc  ");
}

// ---- lines_produced ----

#[test]
fn lines_produced_counts_line_feeds_not_wraps() {
    let mut s = Session::new(4, 3);
    s.feed(b"abcdefghijkl");
    assert_eq!(s.term.lines_produced(), 0);
    s.feed(b"\r\n\r\n");
    assert_eq!(s.term.lines_produced(), 2);
    s.feed(b"\x1bD\x1bE");
    assert_eq!(s.term.lines_produced(), 4);
    let before = s.term.lines_produced();
    s.feed(b"\x1b[?1049h\x1b[?1049l\x1b[2J");
    assert_eq!(s.term.lines_produced(), before);
    s.feed(b"\x1b[3S");
    assert_eq!(s.term.lines_produced(), before + 3);
}

#[test]
fn delete_lines_at_the_top_add_their_history_rows() {
    let mut s = Session::new(4, 4);
    s.feed(b"\x1b[1;1H\x1b[2M");
    assert_eq!(s.term.lines_produced(), 2);
}

// ---- Selection invalidation ----

fn selected_row_zero() -> Session {
    let mut s = Session::new(10, 4);
    s.feed(b"hello");
    s.term
        .selection_start(s.pos(0, 0), Side::Left, SelectionKind::Simple);
    s.term.selection_update(s.pos(0, 3), Side::Right);
    assert!(s.term.has_selection());
    s
}

#[test]
fn in_place_erases_release_a_selection_they_would_blank() {
    for sequence in [
        &b"\x1b[K"[..],
        b"\x1b[1K",
        b"\x1b[2K",
        b"\x1b[J",
        b"\x1b[1J",
        b"\x1b[2J",
        b"\x1b#8",
        b"\x1b[?3h",
        b"\x1b[?3l",
        b"\x1bc",
        b"\x1b[?1049h",
        b"\x1b[?47h",
        b"\x1b[?1047h",
    ] {
        let mut s = selected_row_zero();
        s.feed(b"\x1b[1;1H");
        s.feed(sequence);
        assert!(s.term.state.selection.is_none(), "{sequence:?}");
    }
}

#[test]
fn erases_elsewhere_keep_the_selection() {
    let mut s = selected_row_zero();
    s.feed(b"\x1b[3;1H\x1b[K");
    assert!(s.term.state.selection.is_some());
    s.feed(b"\x1b[3;1H\x1b[J");
    assert!(s.term.state.selection.is_some());
}

#[test]
fn erase_above_reaches_a_selection_on_the_cursor_row_and_below() {
    let mut s = selected_row_zero();
    s.feed(b"\x1b[3;1H\x1b[1J");
    assert!(s.term.state.selection.is_none());
}

#[test]
fn erase_history_releases_only_history_selections() {
    let mut s = Session::new(10, 3);
    s.feed(b"1\n2\n3\n4\n5");
    let history = s.term.screen().screen_top() - 1;
    s.term.selection_start(
        Pos {
            row: history,
            col: 0,
        },
        Side::Left,
        SelectionKind::Lines,
    );
    s.feed(b"\x1b[3J");
    assert!(s.term.state.selection.is_none());
    s.term
        .selection_start(s.pos(0, 0), Side::Left, SelectionKind::Lines);
    s.feed(b"\x1b[3J");
    assert!(s.term.state.selection.is_some());
}

// ---- Modes ----

fn mode_query(s: &mut Session, number: u16) -> String {
    s.feed(format!("\x1b[?{number}$p").as_bytes());
    s.replies()
}

#[test]
fn mouse_protocols_and_encodings_are_exclusive_on_set_only() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b[?1000h\x1b[?1002h");
    assert!(s.term.mode(Mode::MouseDrag) && !s.term.mode(Mode::MouseClick));
    s.feed(b"\x1b[?9h\x1b[?1003h");
    assert!(s.term.mode(Mode::MouseMotion));
    assert!(!s.term.mode(Mode::MouseX10) && !s.term.mode(Mode::MouseDrag));
    s.feed(b"\x1b[?1000h\x1b[?1003l");
    assert!(s.term.mode(Mode::MouseClick), "resetting another leaves it");
    s.feed(b"\x1b[?1006h\x1b[?1015h");
    assert!(s.term.mode(Mode::UrxvtMouse) && !s.term.mode(Mode::SgrMouse));
    s.feed(b"\x1b[?1005h\x1b[?1015l");
    assert!(s.term.mode(Mode::Utf8Mouse));
}

#[test]
fn origin_mode_homes_on_set_and_reset() {
    let mut s = Session::new(10, 10);
    s.feed(b"\x1b[3;7r\x1b[5;5H\x1b[?6h");
    assert_eq!(s.cursor(), (2, 0));
    s.feed(b"\x1b[5;5H\x1b[?6l");
    assert_eq!(s.cursor(), (0, 0));
}

#[test]
fn column_mode_clears_and_keeps_the_width() {
    let mut s = Session::new(10, 4);
    s.feed(b"abc\x1b[2;3r\x1b[?3h");
    assert_eq!(s.row(0).trim_end(), "");
    assert_eq!(s.term.size(), Size { rows: 4, cols: 10 });
    assert_eq!(s.cursor(), (0, 0));
    assert_eq!(s.term.screen().region().bottom, 4);
    assert!(!s.term.mode(Mode::DecCoLm));
    s.feed(b"x\x1b[?3l");
    assert_eq!(s.row(0).trim_end(), "");
}

#[test]
fn reverse_video_changes_no_cell() {
    let mut s = Session::new(10, 2);
    s.feed(b"abc\x1b[?5h");
    assert!(s.term.mode(Mode::ReverseVideo));
    assert_eq!(s.row(0).trim_end(), "abc");
    s.feed(b"\x1b[?5l");
    assert!(!s.term.mode(Mode::ReverseVideo));
}

#[test]
fn synchronized_output_is_stored() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b[?2026h");
    assert!(s.term.mode(Mode::SyncUpdate));
    assert_eq!(mode_query(&mut s, 2026), "\x1b[?2026;1$y");
}

#[test]
fn insert_mode_shifts_text_right() {
    let mut s = Session::new(10, 2);
    s.feed(b"abc\x1b[1;1H\x1b[4hxy\x1b[4lz");
    assert_eq!(s.row(0).trim_end(), "xyzbc");
}

#[test]
fn autowrap_off_overwrites_the_last_column() {
    let mut s = Session::new(4, 2);
    s.feed(b"\x1b[?7labcdef");
    assert_eq!(s.row(0), "abcf");
    assert_eq!(s.row(1), "    ");
}

// ---- Alternate screens ----

#[test]
fn mode_1049_saves_the_cursor_and_wipes() {
    let mut s = Session::new(10, 4);
    s.feed(b"abc\x1b[?1049h");
    assert!(s.term.grid().alt_active());
    assert_eq!(s.cursor(), (0, 3));
    assert_eq!(s.row(0).trim_end(), "");
    s.feed(b"\x1b[3;3Hxyz\x1b[?1049l");
    assert!(!s.term.grid().alt_active());
    assert_eq!(s.cursor(), (0, 3));
    assert_eq!(s.row(0).trim_end(), "abc");
    assert_eq!(s.term.lines_produced(), 0);
}

#[test]
fn mode_1049_when_already_active_does_nothing() {
    let mut s = Session::new(10, 4);
    s.feed(b"\x1b[?1049hxy\x1b[?1049h");
    assert_eq!(s.row(0).trim_end(), "xy");
}

#[test]
fn decsc_survives_mode_47() {
    let mut s = Session::new(10, 4);
    s.feed(b"\x1b[3;4H\x1b7\x1b[?47hxyz\x1b[?47l");
    assert_eq!(s.cursor(), (2, 3), "mode 47 saves and restores nothing");
    s.feed(b"\x1b[1;1H\x1b8");
    assert_eq!(s.cursor(), (2, 3));
}

#[test]
fn mode_47_keeps_the_alternate_content_and_1047_wipes_on_leave() {
    let mut s = Session::new(10, 4);
    s.feed(b"\x1b[?47hxy\x1b[?47l\x1b[?47h");
    assert_eq!(s.row(0).trim_end(), "xy");
    let mut s = Session::new(10, 4);
    s.feed(b"\x1b[?1047hxy\x1b[?1047l\x1b[?1047h");
    assert_eq!(s.row(0).trim_end(), "");
}

#[test]
fn keyboard_flags_swap_with_the_screens() {
    let mut s = Session::new(10, 4);
    s.feed(b"\x1b[>1u\x1b[?1049h");
    s.feed(b"\x1b[?u");
    assert_eq!(s.replies(), "\x1b[?0u");
    s.feed(b"\x1b[>4u\x1b[?u");
    assert_eq!(s.replies(), "\x1b[?4u");
    s.feed(b"\x1b[?1049l\x1b[?u");
    assert_eq!(s.replies(), "\x1b[?1u");
}

#[test]
fn alternate_screen_scrolls_count_but_keep_no_history() {
    let mut s = Session::new(10, 3);
    s.feed(b"\x1b[?1049h");
    let stats = s.feed(b"1\n2\n3\n4\n5");
    assert_eq!(stats.rows_scrolled, 2);
    assert_eq!(s.term.screen().history_len(), 0);
}

// ---- Resets ----

#[test]
fn ris_returns_to_power_on_and_keeps_ids_and_the_line_count() {
    let mut s = Session::new(10, 4);
    s.feed(b"\x1b[?1h\x1b]2;title\x07\x1b[22t\x1b)0\x0e\x1b]4;1;rgb:ff/00/00\x07");
    s.feed(b"\x1b]8;;http://x\x07link\x1b[5 q\n\n");
    assert_eq!(s.term.title_depth(), 1);
    assert!(!s.term.interner().hyperlinks.is_empty());
    let newest = s.term.screen().newest();
    let lines = s.term.lines_produced();
    s.feed(b"\x1bc");
    assert_eq!(s.events()[0], &VtEvent::ScreenCleared);
    assert!(!s.term.mode(Mode::AppCursor));
    assert!(s.term.mode(Mode::LineWrap));
    assert_eq!(s.term.title(), None);
    assert_eq!(s.term.title_depth(), 0);
    assert_eq!(s.term.active_charset(), Charset::Ascii);
    assert_eq!(s.term.color(ColorKey::Palette(1)), None);
    assert_eq!(s.term.interner().hyperlinks.len(), 0);
    assert_eq!(s.term.interner().extras.entries(), 1);
    assert_eq!(s.term.cursor_style(), CursorStyle::default());
    assert_eq!(s.term.screen().newest(), newest);
    assert_eq!(s.term.lines_produced(), lines);
    assert_eq!(s.cursor(), (0, 0));
    assert_eq!(s.term.template(), Cell::EMPTY);
}

#[test]
fn ris_leaves_the_alternate_screen_and_restores_defaults() {
    let mut s = Session::new(10, 4);
    s.feed(b"\x1b[?1049h\x1b[>3u\x1b[>4;2m\x1b[?1000h\x1bc");
    assert!(!s.term.grid().alt_active());
    assert_eq!(s.term.keyboard_flags(), KeyboardFlags::empty());
    assert_eq!(s.term.modify_other_keys(), 0);
    assert!(!s.term.mode(Mode::MouseClick));
}

#[test]
fn decstr_resets_exactly_what_it_lists() {
    let mut s = Session::new(10, 6);
    s.feed(b"keep\x1b[1;31m\x1b[?6h\x1b[4h\x1b[?7l\x1b[?25l\x1b[2;5r\x1b[3;3H");
    s.feed(b"\x1b]2;title\x07\x1b]4;1;rgb:ff/00/00\x07\x1b)0\x0e\x1b[?1h\x1b[?1000h\x1b[6 q");
    s.feed(b"\x1b[>5u\x1b[>4;1m");
    s.feed(b"\x1b[!p");
    assert!(!s.term.mode(Mode::Origin));
    assert!(!s.term.mode(Mode::Insert));
    assert!(s.term.mode(Mode::LineWrap));
    assert!(s.term.mode(Mode::ShowCursor));
    assert_eq!(s.term.template(), Cell::EMPTY);
    assert_eq!(s.term.screen().region().top, 0);
    assert_eq!(s.term.screen().region().bottom, 6);
    assert_eq!(s.cursor(), (0, 0));
    // Kept.
    assert_eq!(s.row(0).trim_end(), "keep");
    assert_eq!(s.term.title(), Some("title"));
    assert!(s.term.color(ColorKey::Palette(1)).is_some());
    assert_eq!(
        s.term.active_charset(),
        Charset::SpecialCharacterAndLineDrawing
    );
    assert!(s.term.mode(Mode::AppCursor));
    assert!(s.term.mode(Mode::MouseClick));
    assert_eq!(
        s.term.cursor_style(),
        CursorStyle {
            shape: CursorShape::Beam,
            blinking: false
        }
    );
    assert_eq!(s.term.keyboard_flags().bits(), 5);
    assert_eq!(s.term.modify_other_keys(), 1);
    // DECSC ran: DECRC returns to the reset cursor.
    s.feed(b"\x1b[4;4H\x1b8");
    assert_eq!(s.cursor(), (0, 0));
}

// ---- Cursor style ----

#[test]
fn decscusr_selects_every_style() {
    for (selector, shape, blinking) in [
        (0, CursorShape::Block, false),
        (1, CursorShape::Block, true),
        (2, CursorShape::Block, false),
        (3, CursorShape::Underline, true),
        (4, CursorShape::Underline, false),
        (5, CursorShape::Beam, true),
        (6, CursorShape::Beam, false),
    ] {
        let mut s = Session::new(10, 2);
        s.feed(format!("\x1b[{selector} q").as_bytes());
        assert_eq!(s.term.cursor_style(), CursorStyle { shape, blinking });
    }
    let mut s = Session::new(10, 2);
    assert_eq!(s.feed(b"\x1b[9 q").unhandled_sequences, 1);
    assert_eq!(s.term.cursor_style(), CursorStyle::default());
}

#[test]
fn hidden_cursor_keeps_shape_and_blink_underneath() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b[5 q\x1b[?25l");
    assert_eq!(
        s.term.cursor_style(),
        CursorStyle {
            shape: CursorShape::Hidden,
            blinking: true
        }
    );
    s.feed(b"\x1b[?25h");
    assert_eq!(s.term.cursor_style().shape, CursorShape::Beam);
}

#[test]
fn blink_mode_edits_the_stored_style() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b[?12h");
    assert!(s.term.mode(Mode::CursorBlink));
    assert!(s.term.cursor_style().blinking);
    s.feed(b"\x1b[?12l");
    assert!(!s.term.mode(Mode::CursorBlink));
}

#[test]
fn cursor_style_changed_fires_exactly_on_a_change() {
    let mut s = Session::new(10, 2);
    let changed = |s: &Session| s.events().contains(&&VtEvent::CursorStyleChanged);
    s.feed(b"\x1b[0 q");
    assert!(!changed(&s));
    s.feed(b"\x1b[2 q");
    assert!(!changed(&s), "steady block is the default");
    s.feed(b"\x1b[1 q");
    assert!(changed(&s));
    s.feed(b"\x1b[1 q");
    assert!(!changed(&s));
    s.feed(b"\x1b[?25l");
    assert!(changed(&s));
    s.feed(b"\x1b[?25l");
    assert!(!changed(&s));
    s.feed(b"\x1b[?25h");
    assert!(changed(&s));
    s.feed(b"\x1b[?12l");
    assert!(changed(&s));
    s.feed(b"\x1b[!p");
    assert!(!changed(&s));
    s.feed(b"\x1b[?25l");
    s.feed(b"\x1b[!p");
    assert!(changed(&s), "DECSTR shows the cursor again");
    s.feed(b"\x1b[?25l");
    s.feed(b"\x1bc");
    assert!(changed(&s));
    s.feed(b"\x1bc");
    assert!(!changed(&s));
}

#[test]
fn configured_default_style_is_the_baseline() {
    let config = Config {
        default_cursor_style: CursorStyle {
            shape: CursorShape::Underline,
            blinking: true,
        },
        ..Config::default()
    };
    let mut s = Session::with(config);
    assert_eq!(s.term.cursor_style().shape, CursorShape::Underline);
    s.feed(b"\x1b[0 q");
    assert_eq!(s.term.cursor_style().shape, CursorShape::Underline);
    s.feed(b"\x1b[2 q");
    assert_eq!(s.term.cursor_style().shape, CursorShape::Block);
}

// ---- Kitty keyboard and modifyOtherKeys ----

#[test]
fn kitty_flags_push_pop_and_set() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b[>5u");
    assert_eq!(
        s.term.keyboard_flags(),
        KeyboardFlags::DISAMBIGUATE_ESC_CODES | KeyboardFlags::REPORT_ALTERNATE_KEYS
    );
    s.feed(b"\x1b[>2u");
    assert_eq!(s.term.keyboard_flags(), KeyboardFlags::REPORT_EVENT_TYPES);
    s.feed(b"\x1b[<u");
    assert_eq!(s.term.keyboard_flags().bits(), 5);
    s.feed(b"\x1b[=2;2u");
    assert_eq!(s.term.keyboard_flags().bits(), 7);
    s.feed(b"\x1b[=1;3u");
    assert_eq!(s.term.keyboard_flags().bits(), 6);
    s.feed(b"\x1b[=8u");
    assert_eq!(s.term.keyboard_flags().bits(), 8);
    s.feed(b"\x1b[<u");
    assert_eq!(
        s.term.keyboard_flags().bits(),
        0,
        "the stack held one entry"
    );
    s.feed(b"\x1b[>255u");
    assert_eq!(s.term.keyboard_flags().bits(), 0x1F);
}

#[test]
fn kitty_pop_past_the_depth_empties_and_twelve_pushes_keep_eight() {
    let mut s = Session::new(10, 2);
    for flags in 1..=12 {
        s.feed(format!("\x1b[>{flags}u").as_bytes());
    }
    s.feed(b"\x1b[<7u");
    assert_eq!(
        s.term.keyboard_flags().bits(),
        5,
        "entries 5..=12 were kept"
    );
    s.feed(b"\x1b[<99u");
    assert_eq!(s.term.keyboard_flags().bits(), 0);
    s.feed(b"\x1b[<u");
    assert_eq!(s.term.keyboard_flags().bits(), 0);
}

#[test]
fn modify_other_keys_levels() {
    let mut s = Session::new(10, 2);
    s.feed(b"\x1b[>4;2m");
    assert_eq!(s.term.modify_other_keys(), 2);
    s.feed(b"\x1b[>4;0m");
    assert_eq!(s.term.modify_other_keys(), 0);
    s.feed(b"\x1b[>4m");
    assert_eq!(s.term.modify_other_keys(), 0);
    for sequence in [&b"\x1b[>4;3m"[..], b"\x1b[>3;1m", b"\x1b[>m", b"\x1b[?3m"] {
        assert_eq!(s.feed(sequence).unhandled_sequences, 1, "{sequence:?}");
    }
    assert_eq!(s.term.modify_other_keys(), 0);
}

// ---- Grapheme sweep ----

fn cluster(i: usize) -> String {
    let letter = char::from(b'a' + (i % 26) as u8);
    let first = char::from_u32(0x300 + (i / 26 % 112) as u32).expect("combining mark");
    let second = char::from_u32(0x300 + (i / (26 * 112) % 112) as u32).expect("combining mark");
    format!("{letter}{first}{second}")
}

#[test]
fn grapheme_sweep_reclaims_dead_ids_and_restamps_live_rows() {
    let mut s = Session::new(80, 10);
    s.feed("a\u{301}b\u{302}".as_bytes());
    let top = s.term.screen().screen_top();
    s.feed(b"\x1b[?47h");
    s.feed(b"\r");
    let stamp_before = s.term.primary_row_seq(top);
    // 70,000 distinct clusters on the alternate screen: the arena passes its sweep threshold.
    let stream: String = (0..70_000).map(cluster).collect();
    let (mut previous, mut swept) = (0, false);
    for chunk in stream.as_bytes().chunks(60_000) {
        s.feed(chunk);
        let entries = s.term.interner().graphemes.entries();
        swept |= entries < previous;
        previous = entries;
    }
    assert!(swept, "the arena shrank at least once");
    assert!(
        s.term.interner().graphemes.entries() < 100_000,
        "without sweeps the arena would hold two entries per cluster: {}",
        s.term.interner().graphemes.entries()
    );
    let expected: Vec<String> = (0..10)
        .map(|row| {
            (69_200 + row * 80..69_200 + (row + 1) * 80)
                .map(cluster)
                .collect()
        })
        .collect();
    let rows: Vec<String> = (0..10).map(|row| s.row(row)).collect();
    assert_eq!(rows, expected);
    s.feed(b"\x1b[?47l");
    assert!(s.row(0).starts_with("a\u{301}b\u{302}"));
    assert!(s.term.primary_row_seq(top) > stamp_before);
}

#[test]
fn grapheme_sweep_carries_counters_over() {
    let mut s = Session::new(80, 4);
    let long = format!("e{}", "\u{301}".repeat(16));
    assert_eq!(s.feed(long.as_bytes()).grapheme_truncated, 1);
    let stream: String = (0..70_000).map(cluster).collect();
    let stats = s.feed(stream.as_bytes());
    assert_eq!(
        stats.grapheme_truncated, 0,
        "the carried counter is not re-counted"
    );
    assert_eq!(s.term.interner().graphemes.truncated(), 1);
}

#[test]
fn the_erase_cell_follows_only_the_pen_background() {
    let mut s = Session::new(6, 2);
    s.feed(b"\x1b[44m");
    let erase = s.term.screen().cursor().erase();
    let style = *s.term.interner().resolve_style(erase.style_id());
    assert_ne!(style.bg, Style::DEFAULT.bg);
    // Attribute changes and a repeated SGR keep the background, so they keep the erase cell.
    s.feed(b"\x1b[1m\x1b[1m\x1b[44m");
    assert_eq!(s.term.screen().cursor().erase(), erase);
    s.feed(b"\x1b[49m");
    assert_eq!(s.term.screen().cursor().erase(), Cell::EMPTY);
}

#[test]
fn encode_key_reads_the_terminals_modes() {
    use crate::input::{KeyMods, KeySpec, NamedKey};

    let mut s = Session::new(80, 24);
    let up = KeySpec::Named(NamedKey::ArrowUp);
    let escape = KeySpec::Named(NamedKey::Escape);
    let ctrl_a = KeySpec::Character("a".to_owned());
    let ctrl = KeyMods {
        ctrl: true,
        ..KeyMods::default()
    };
    assert_eq!(
        s.term.encode_key(&up, KeyMods::default()).as_deref(),
        Some(&b"\x1b[A"[..])
    );
    s.feed(b"\x1b[?1h");
    assert_eq!(
        s.term.encode_key(&up, KeyMods::default()).as_deref(),
        Some(&b"\x1bOA"[..])
    );
    s.feed(b"\x1b[>1u");
    assert_eq!(
        s.term.encode_key(&escape, KeyMods::default()).as_deref(),
        Some(&b"\x1b[27u"[..])
    );
    s.feed(b"\x1b[<u\x1b[>4;2m");
    assert_eq!(
        s.term.encode_key(&ctrl_a, ctrl).as_deref(),
        Some(&b"\x1b[27;5;97~"[..])
    );
    s.feed(b"\x1bc");
    assert_eq!(
        s.term.encode_key(&ctrl_a, ctrl).as_deref(),
        Some(&b"\x01"[..])
    );
}

#[test]
fn encode_key_event_reports_a_release() {
    use crate::input::{KeyEventKind, KeyMods, KeySpec};

    let mut s = Session::new(80, 24);
    let mut event = KeyEvent::new(KeySpec::Character("a".to_owned()), KeyMods::default());
    event.kind = KeyEventKind::Release;
    assert_eq!(s.term.encode_key_event(&event), None);
    s.feed(b"\x1b[>10u");
    assert_eq!(
        s.term.encode_key_event(&event).as_deref(),
        Some(&b"\x1b[97;1:3u"[..])
    );
}
