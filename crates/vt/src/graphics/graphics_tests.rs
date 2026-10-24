//! Tests of placements and graphic lifetime, driven through a real `Terminal`.

use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::events::{EventBatch, FeedStats, VtEvent};
use crate::grid::terminal_grid::ResizePolicy;
use crate::grid::{Pos, RowId, Size};
use crate::snapshot::SnapshotState;
use crate::terminal::{Config, Terminal};

/// A terminal with its batch and one snapshot state.
struct Session {
    term: Terminal,
    batch: EventBatch,
    state: SnapshotState,
    now: Instant,
}

impl Session {
    fn new(cols: u16, rows: u16) -> Session {
        Session::with(cols, rows, Config::default())
    }

    fn with(cols: u16, rows: u16, config: Config) -> Session {
        Session {
            term: Terminal::new(Size { rows, cols }, config),
            batch: EventBatch::new(),
            state: SnapshotState::new(),
            now: Instant::now(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> FeedStats {
        self.term.feed(bytes, &mut self.batch, self.now)
    }

    fn feed_str(&mut self, text: &str) -> FeedStats {
        self.feed(text.as_bytes())
    }

    /// Feeds a Sixel image `width` x `height` px made of `bands` bands.
    fn image(&mut self, width: u32, height: u32, bands: u32) -> FeedStats {
        self.feed(&image(width, height, bands))
    }

    /// The images decoded since the last call; exactly one is expected.
    fn one_image(&mut self) -> Arc<GraphicData> {
        let mut taken = self.term.take_graphics();
        assert_eq!(taken.len(), 1, "exactly one image was pending");
        taken.remove(0)
    }

    /// The cursor as `(screen row index, column)`.
    fn cursor(&self) -> (u16, u16) {
        let screen = self.term.screen();
        (screen.cursor_row_index(), screen.cursor().pos.col)
    }

    /// The ids released by the last feed, in order.
    fn released(&self) -> Vec<GraphicId> {
        self.batch
            .iter()
            .filter_map(|event| match event {
                VtEvent::GraphicReleased(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// The image named by the cell at screen row `index`, column `col`.
    fn graphic(&self, index: u16, col: u16) -> Option<GraphicId> {
        let screen = self.term.screen();
        self.graphic_at(screen.row_of_index(index), col)
    }

    fn graphic_at(&self, row: RowId, col: u16) -> Option<GraphicId> {
        let cell = self.term.screen().row(row).cell(col);
        self.term
            .interner()
            .resolve_extras(cell.extras_id())
            .graphic
    }

    /// `(image, dx, dy)` of the cell at viewport row `index`, column `col`, as a renderer
    /// finds it through the snapshot.
    fn offset(&mut self, index: u16, col: u16) -> Option<(GraphicId, u16, u16)> {
        self.term.snapshot_update(&mut self.state, self.now);
        let row = &self.state.rows()[usize::from(index)];
        let id = row.cells[usize::from(col)].graphic?;
        let (dx, dy) = self.state.graphic_offset(id, row.id, col)?;
        Some((id, dx, dy))
    }

    fn anchor(&self, index: usize) -> Option<Pos> {
        let placement = self.term.placements().get(index)?;
        self.term.grid().anchors().get(placement.anchor)
    }

    fn anchor_index(&self, index: usize) -> Option<u16> {
        self.term.screen().index_of(self.anchor(index)?.row)
    }

    fn replies(&self) -> String {
        let mut bytes = Vec::new();
        for event in self.batch.iter() {
            if let VtEvent::Reply(span) = event {
                bytes.extend_from_slice(self.batch.bytes(*span));
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn row_text(&self, index: u16) -> String {
        self.term.row_text(self.term.screen().row_of_index(index))
    }
}

/// `ESC P q <body> ESC \`.
fn sixel(body: &str) -> Vec<u8> {
    [b"\x1bPq", body.as_bytes(), b"\x1b\\"].concat()
}

/// A sixel image declared `width` x `height` px with `bands` bands of one column each.
fn image(width: u32, height: u32, bands: u32) -> Vec<u8> {
    let data = vec!["~"; bands.max(1) as usize].join("-");
    sixel(&format!("\"1;1;{width};{height}{data}"))
}

/// A one-pixel image: one cell at the default cell size.
fn dot() -> Vec<u8> {
    image(1, 1, 1)
}

fn cup(row: u16, col: u16) -> String {
    format!("\x1b[{};{}H", row + 1, col + 1)
}

/// Feeds enough unique OSC 8 links to run the extras sweep at least once.
fn churn_links(session: &mut Session, count: usize) {
    let mut bytes = Vec::new();
    for index in 0..count {
        bytes.extend_from_slice(format!("\x1b]8;;http://example.test/{index}\x07").as_bytes());
    }
    session.feed(&bytes);
}

// ---- geometry ----

#[test]
fn placed_at_cursor_column_clipped_right() {
    let mut s = Session::new(20, 10);
    s.feed_str(&cup(2, 3));
    s.image(25, 45, 8);
    let placement = s.term.placements()[0];
    assert_eq!((placement.cols, placement.rows), (3, 3));
    assert_eq!(placement.pixel_size, (25, 45));
    assert_eq!(
        s.anchor(0),
        Some(Pos {
            row: s.term.screen().row_of_index(2),
            col: 3
        })
    );
    assert_eq!(s.graphic(2, 3), Some(placement.id));
    assert_eq!(s.graphic(2, 5), Some(placement.id));
    assert_eq!(s.graphic(2, 6), None);
    assert_eq!(s.graphic(2, 2), None);
    assert_eq!(s.graphic(4, 4), Some(placement.id));
    assert_eq!(s.graphic(5, 4), None);

    let mut narrow = Session::new(4, 4);
    narrow.feed_str("abcd");
    narrow.feed_str(&cup(0, 2));
    narrow.image(80, 20, 1);
    assert_eq!(narrow.term.placements()[0].cols, 2);
    assert_eq!(narrow.row_text(0), "abcd");
    assert!(narrow.graphic(0, 1).is_none());
    assert!(narrow.graphic(0, 3).is_some());
}

#[test]
fn cursor_walk_is_band_pixels_over_cell_height() {
    for (bands, down) in [(1, 0), (4, 0), (5, 1), (8, 2)] {
        let mut s = Session::new(20, 10);
        s.image(60, 6 * bands, bands);
        assert_eq!(s.cursor(), (down, 0), "{bands} bands");
    }
}

#[test]
fn footprint_follows_cell_pixels() {
    // 96 bands of 6 px: 570 px above the last band.
    for (cell, footprint, row) in [
        (Some((9, 18)), (43, 32), 31),
        (Some((18, 36)), (22, 16), 15),
        (None, (39, 29), 28),
    ] {
        let mut s = Session::new(100, 50);
        if let Some((width, height)) = cell {
            s.term.set_cell_pixels(width, height);
        }
        s.image(384, 576, 96);
        let placement = s.term.placements()[0];
        assert_eq!((placement.cols, placement.rows), footprint, "{cell:?}");
        assert_eq!(s.cursor(), (row, 0), "{cell:?}");
    }
}

#[test]
fn one_pixel_image_is_one_cell() {
    let mut s = Session::new(10, 5);
    s.feed(&dot());
    let placement = s.term.placements()[0];
    assert_eq!((placement.cols, placement.rows), (1, 1));
    assert!(s.graphic(0, 0).is_some() && s.graphic(0, 1).is_none() && s.graphic(1, 0).is_none());
}

#[test]
fn zero_axis_falls_back() {
    for cell in [(9, 0), (0, 18), (0, 0)] {
        let mut s = Session::new(40, 10);
        s.term.set_cell_pixels(cell.0, cell.1);
        s.image(100, 40, 7);
        let placement = s.term.placements()[0];
        assert_eq!((placement.cols, placement.rows), (10, 2), "{cell:?}");
    }
}

#[test]
fn footprint_is_ceil() {
    for (pixels, cells) in [(1, 1), (7, 1), (8, 1), (9, 2), (16, 2), (17, 3)] {
        let mut s = Session::new(40, 40);
        s.term.set_cell_pixels(8, 8);
        s.image(pixels, pixels, 1);
        let placement = s.term.placements()[0];
        assert_eq!(
            (placement.cols, placement.rows),
            (cells, cells),
            "{pixels} px"
        );
    }
}

#[test]
fn cursor_keeps_column() {
    let mut s = Session::new(20, 10);
    s.feed_str(&cup(1, 7));
    s.image(10, 60, 10);
    assert_eq!(s.cursor(), (1 + 2, 7));
}

#[test]
fn taller_than_screen_scrolls_and_stays_anchored() {
    let mut s = Session::new(20, 10);
    let stats = s.image(20, 400, 67);
    let placement = s.term.placements()[0];
    assert_eq!(placement.rows, 20);
    // 66 bands above the last: 396 px, 19 line feeds on a 10-row screen.
    assert_eq!(stats.rows_scrolled, 10);
    assert_eq!(s.term.lines_produced(), 19);
    let top = s.anchor(0).expect("the anchor follows the content");
    assert_eq!(top.row, RowId(0));
    assert!(s.term.screen().history_len() >= 10);
    assert_eq!(s.graphic_at(RowId(0), 0), Some(placement.id));
    assert_eq!(s.graphic(9, 0), Some(placement.id));
    assert!(s.released().is_empty());
    assert_eq!(s.term.placements().len(), 1);
}

#[test]
fn wider_than_screen_clipped_not_wrapped() {
    let mut s = Session::new(80, 10);
    s.image(4000, 20, 1);
    assert_eq!(s.term.placements()[0].cols, 80);
    assert_eq!(s.cursor(), (0, 0));
    assert!(
        !s.term
            .screen()
            .row(s.term.screen().row_of_index(0))
            .wrapped()
    );
}

#[test]
fn height_not_multiple_of_six() {
    let mut s = Session::new(20, 10);
    s.feed(&sixel("\"1;1;4;10~-~"));
    let image = s.one_image();
    assert_eq!((image.width, image.height), (4, 10));
    assert_eq!(image.rgba.len(), 4 * 10 * 4);
}

#[test]
fn raster_larger_than_data_cursor_inside() {
    let mut s = Session::new(20, 10);
    s.feed(&sixel("\"1;1;10;100~"));
    let placement = s.term.placements()[0];
    assert_eq!((placement.cols, placement.rows), (1, 5));
    assert_eq!(s.cursor(), (0, 0));
}

#[test]
fn raster_smaller_than_data_cursor_past() {
    let mut s = Session::new(20, 30);
    let body = format!("\"1;1;10;20{}", vec!["~"; 40].join("-"));
    s.feed(&sixel(&body));
    let placement = s.term.placements()[0];
    assert_eq!(placement.rows, 1);
    // 39 bands above the last: 234 px, 11 rows.
    assert_eq!(s.cursor(), (11, 0));
}

#[test]
fn cell_change_keeps_earlier_footprint() {
    let mut s = Session::new(40, 10);
    s.image(100, 40, 1);
    s.term.set_cell_pixels(5, 5);
    let before = s.term.placements()[0];
    assert_eq!((before.cols, before.rows), (10, 2));
    s.feed(b"x");
    assert_eq!(s.term.placements()[0], before);
}

#[test]
fn one_pixel_cell_does_not_hang() {
    let mut s = Session::new(80, 24);
    s.term.set_cell_pixels(1, 1);
    s.feed(&sixel("\"1;1;4096;4096~"));
    let placement = s.term.placements()[0];
    assert_eq!((placement.cols, placement.rows), (80, 4096));
    let mut walked = Session::new(80, 24);
    walked.term.set_cell_pixels(1, 1);
    walked.feed(&image(8, 4000, 700));
    assert_eq!(walked.term.placements().len(), 1);
}

#[test]
fn trailing_graphics_newline_moves_below() {
    let mut s = Session::new(100, 50);
    s.term.set_cell_pixels(9, 18);
    let body = format!("\"1;1;384;576{}-", vec!["~"; 96].join("-"));
    s.feed(&sixel(&body));
    assert_eq!(s.cursor(), (32, 0));
}

// ---- walk side effects ----

#[test]
fn walk_counts_lines_and_scroll_events() {
    let mut s = Session::new(20, 5);
    s.feed_str(&cup(4, 0));
    let stats = s.image(10, 66, 11);
    // 60 px above the last band: 3 line feeds at the bottom of the screen.
    assert_eq!(s.term.lines_produced(), 3);
    assert_eq!(stats.rows_scrolled, 3);
    assert_eq!(s.cursor(), (4, 0));
}

#[test]
fn walk_honours_scroll_region() {
    let mut s = Session::new(20, 8);
    s.feed_str("\x1b[3;5r");
    s.feed_str(&cup(4, 0));
    s.image(10, 66, 11);
    let scrolls = s
        .batch
        .iter()
        .filter(|event| matches!(event, VtEvent::RowsScrolled(_)))
        .count();
    assert_eq!(scrolls, 3);
    assert_eq!(s.term.lines_produced(), 3);
    assert_eq!(
        s.term.stats().rows_scrolled,
        0,
        "a region below the top feeds no history"
    );
    assert_eq!(s.cursor(), (4, 0));
}

// ---- cells ----

#[test]
fn one_extras_entry_per_image() {
    let mut s = Session::new(400, 210);
    s.term.set_cell_pixels(10, 20);
    let before = s.term.interner().extras.entries();
    s.image(4000, 4000, 1);
    assert_eq!(s.term.interner().extras.entries(), before + 1);
    let placement = s.term.placements()[0];
    assert_eq!((placement.cols, placement.rows), (400, 200));
    assert!(s.graphic(0, 399).is_some() && s.graphic(199, 0).is_some());
}

#[test]
fn two_images_stay_distinct() {
    let mut s = Session::new(20, 10);
    s.image(10, 20, 1);
    s.feed_str(&cup(0, 5));
    s.image(10, 20, 1);
    let [first, second] = [s.term.placements()[0].id, s.term.placements()[1].id];
    assert_ne!(first, second);
    assert_eq!(s.graphic(0, 0), Some(first));
    assert_eq!(s.graphic(0, 5), Some(second));
}

#[test]
fn text_over_a_cell_drops_the_image_id() {
    let mut s = Session::new(20, 10);
    s.image(30, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str(&cup(0, 1));
    s.feed_str("x");
    assert_eq!(s.graphic(0, 0), Some(id));
    assert_eq!(s.graphic(0, 1), None);
    assert_eq!(s.graphic(0, 2), Some(id));
    assert_eq!(s.row_text(0).trim_end(), " x");
    assert_eq!(s.term.placements().len(), 1, "the row flag keeps it live");
}

#[test]
fn ed2_primary_keeps_image_in_history() {
    let mut s = Session::new(20, 5);
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\x1b[2J");
    assert!(s.released().is_empty());
    assert_eq!(s.term.placements().len(), 1);
    assert_eq!(s.graphic_at(RowId(0), 0), Some(id));
}

#[test]
fn merged_link_and_image_keep_both() {
    let mut s = Session::new(20, 5);
    s.feed_str("\x1b]8;;http://a.test\x07abc\x1b]8;;\x07");
    s.feed_str(&cup(0, 0));
    s.image(30, 20, 1);
    let id = s.term.placements()[0].id;
    s.term.snapshot_update(&mut s.state, s.now);
    let cell = s.state.rows()[0].cells[1];
    assert_eq!(cell.graphic, Some(id));
    let link = cell.hyperlink.expect("the link survives the image");
    assert_eq!(
        &*s.state.hyperlink(link).expect("copied").uri,
        "http://a.test"
    );
    assert_eq!(s.row_text(0).trim_end(), "abc");
}

// ---- movement ----

#[test]
fn scrolls_into_history_with_its_cells() {
    let mut s = Session::new(10, 3);
    s.image(10, 40, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\r\n\r\n\r\n\r\n");
    assert_eq!(s.term.placements().len(), 1);
    assert!(s.released().is_empty());
    assert_eq!(s.graphic_at(RowId(0), 0), Some(id));
    assert_eq!(s.graphic_at(RowId(1), 0), Some(id));
    assert_eq!(s.anchor(0).expect("alive in history").row, RowId(0));
}

#[test]
fn moves_with_in_region_scroll() {
    let mut s = Session::new(10, 8);
    s.feed_str(&cup(3, 0));
    s.image(10, 20, 1);
    assert_eq!(s.anchor_index(0), Some(3));
    s.feed_str("\x1b[2S");
    assert_eq!(s.anchor_index(0), Some(1));
    s.feed_str(&cup(0, 0));
    s.feed_str("\x1b[2L");
    assert_eq!(s.anchor_index(0), Some(3));
    assert_eq!(s.term.placements().len(), 1);
}

#[test]
fn reflow_keeps_anchor_on_its_row() {
    let mut s = Session::new(10, 6);
    s.feed_str("one\r\ntwo\r\n");
    s.image(20, 20, 1);
    s.feed_str(&cup(4, 0));
    s.feed_str("after");
    let id = s.term.placements()[0].id;
    for cols in [20, 5, 12] {
        s.term
            .resize(Size { rows: 6, cols }, ResizePolicy::BottomAnchor);
        let pos = s.anchor(0).expect("anchor survives the reflow");
        assert_eq!(s.graphic_at(pos.row, 0), Some(id), "{cols} cols");
        // The anchor sits on the blank row between "two" and "after".
        assert_eq!(s.term.row_text(pos.row).trim(), "", "{cols} cols");
        assert_eq!(s.term.row_text(pos.row - 1).trim(), "two", "{cols} cols");
    }
    s.feed(b"");
    assert!(s.released().is_empty());
    assert_eq!(s.term.placements().len(), 1);
}

// ---- release ----

#[test]
fn released_on_alt_clear() {
    let mut s = Session::new(10, 5);
    s.feed_str("\x1b[?1049h");
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    assert!(s.released().is_empty());
    s.feed_str("\x1b[2J");
    assert_eq!(s.released(), [id]);
    assert!(s.term.placements().is_empty());
    s.feed_str("x");
    assert!(s.released().is_empty(), "exactly once");
}

#[test]
fn released_on_ris() {
    let mut s = Session::new(10, 5);
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\x1bc");
    assert_eq!(s.released(), [id]);
    assert!(s.term.placements().is_empty());
    s.feed_str("y");
    assert!(s.released().is_empty());
}

#[test]
fn released_on_blanking_region_scroll() {
    let mut s = Session::new(10, 8);
    s.feed_str("\x1b[3;6r");
    s.feed_str(&cup(3, 0));
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\x1b[4S");
    assert_eq!(s.released(), [id]);
}

#[test]
fn released_when_anchor_trimmed() {
    let mut s = Session::with(
        10,
        3,
        Config {
            scrollback_limit: 1,
            ..Config::default()
        },
    );
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str(&cup(2, 0));
    s.feed_str("\n");
    assert!(s.released().is_empty(), "still in the one history row");
    s.feed_str("\n");
    assert_eq!(s.released(), [id]);
}

#[test]
fn released_when_ed3_clears_history() {
    let mut s = Session::with(
        10,
        3,
        Config {
            scrollback_limit: 10,
            ..Config::default()
        },
    );
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str(&cup(2, 0));
    s.feed_str(
        "

",
    );
    assert!(s.released().is_empty(), "the image sits in history");
    s.feed_str("[3J");
    assert_eq!(s.released(), [id]);
}

/// A terminal whose resize to two columns pushes the image row out of a history-less screen.
/// Returns it with the image's id; the resize has run and the next feed has not.
fn reflow_kills_anchor() -> (Session, GraphicId) {
    let mut s = Session::with(
        8,
        4,
        Config {
            scrollback_limit: 0,
            ..Config::default()
        },
    );
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\r\naaaaaaaa\r\nbbbbbbbb\r\ncccccccc");
    assert!(s.released().is_empty());
    let outcome = s
        .term
        .resize(Size { rows: 4, cols: 2 }, ResizePolicy::BottomAnchor);
    assert!(outcome.reflowed);
    (s, id)
}

#[test]
fn released_when_reflow_drops_anchor() {
    let (mut s, id) = reflow_kills_anchor();
    assert!(s.term.placements().is_empty(), "swept by the resize");
    s.feed_str("x");
    assert_eq!(s.released(), [id]);
}

#[test]
fn released_on_alt_wipe() {
    let mut s = Session::new(10, 5);
    s.feed_str("\x1b[?1049h");
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\x1b[?1049l");
    assert!(
        s.released().is_empty(),
        "the hidden screen keeps its images"
    );
    assert_eq!(s.term.placements().len(), 1);
    s.feed_str("\x1b[?1049h");
    assert_eq!(s.released(), [id]);
}

#[test]
fn released_on_alt_wipe_when_leaving_with_1047() {
    let mut s = Session::new(10, 5);
    s.feed_str("\x1b[?1047h");
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\x1b[?1047l");
    assert_eq!(s.released(), [id]);
}

#[test]
fn release_delivered_by_empty_feed() {
    let (mut s, id) = reflow_kills_anchor();
    s.feed(b"");
    assert_eq!(s.released(), [id]);
    assert!(!s.batch.iter().any(|event| *event == VtEvent::Repaint));
    s.feed(b"");
    assert!(s.batch.is_empty(), "delivered once");
}

#[test]
fn released_before_repaint() {
    let mut s = Session::new(10, 5);
    s.image(10, 20, 1);
    let id = s.term.placements()[0].id;
    s.feed_str("\x1bc");
    let events: Vec<&VtEvent> = s.batch.iter().collect();
    let released = events
        .iter()
        .position(|event| **event == VtEvent::GraphicReleased(id))
        .expect("released");
    assert_eq!(events.last(), Some(&&VtEvent::Repaint));
    assert!(released < events.len() - 1);
}

#[test]
fn eviction_at_256() {
    let mut s = Session::new(80, 24);
    let mut ids = Vec::new();
    for index in 0..257u16 {
        s.feed_str(&cup(index / 80, index % 80));
        s.feed(&dot());
        ids.push(s.term.placements().last().expect("placed").id);
        if index < 256 {
            assert!(s.released().is_empty(), "room for {} placements", index + 1);
        }
    }
    // The 257th image evicted the oldest placement, in its own feed.
    assert_eq!(s.released(), [ids[0]]);
    assert_eq!(s.term.placements().len(), MAX_PLACEMENTS);
    assert_eq!(s.term.placements()[0].id, ids[1]);
    // Its cell still names it; only the placement went.
    assert_eq!(s.graphic(0, 0), Some(ids[0]));
    s.feed(b"x");
    assert!(s.released().is_empty(), "exactly once");
}

// ---- D3: a newer image covers an older one ----

#[test]
fn newer_image_covering_older_releases_it() {
    let mut s = Session::new(20, 10);
    s.image(10, 20, 1);
    let first = s.term.placements()[0].id;
    s.feed_str(&cup(0, 0));
    s.image(10, 20, 1);
    assert_eq!(s.released(), [first]);
    assert_eq!(s.term.placements().len(), 1);
    assert_ne!(s.term.placements()[0].id, first);
}

#[test]
fn smaller_newer_image_keeps_older() {
    let mut s = Session::new(20, 10);
    s.image(30, 40, 1);
    s.feed_str(&cup(0, 0));
    s.image(10, 20, 1);
    assert!(s.released().is_empty());
    assert_eq!(s.term.placements().len(), 2);
}

#[test]
fn cover_ignores_other_lane() {
    let mut s = Session::new(20, 10);
    s.image(10, 20, 1);
    s.feed_str("\x1b[?1049h");
    s.image(10, 20, 1);
    assert!(s.released().is_empty());
    assert_eq!(s.term.placements().len(), 2);
}

// ---- no recycling ----

#[test]
fn evicted_screen_cell_not_recycled_into_link() {
    let mut s = Session::new(80, 24);
    s.image(10, 20, 1);
    let first = s.term.placements()[0].id;
    for index in 1..=256u16 {
        s.feed_str(&cup(index / 80 % 24 + 1, index % 80));
        s.feed(&dot());
    }
    assert!(s.term.placements().iter().all(|p| p.id != first), "evicted");
    churn_links(&mut s, 4200);
    assert_eq!(s.graphic(0, 0), Some(first));
}

#[test]
fn evicted_history_cell_not_recycled() {
    let mut s = Session::new(80, 24);
    s.image(10, 20, 1);
    let first = s.term.placements()[0].id;
    s.feed_str(&cup(23, 0));
    s.feed_str(&"\n".repeat(30));
    assert!(s.term.screen().history_len() >= 6);
    for index in 1..=256u16 {
        s.feed_str(&cup(index / 80 % 20, index % 80));
        s.feed(&dot());
    }
    assert!(s.term.placements().iter().all(|p| p.id != first), "evicted");
    churn_links(&mut s, 4200);
    assert_eq!(s.graphic_at(RowId(0), 0), Some(first));
}

#[test]
fn swept_row_not_recycled() {
    let mut s = Session::new(20, 10);
    s.feed_str("\x1b[2;10r");
    s.feed_str(&cup(1, 0));
    s.image(10, 60, 1);
    let id = s.term.placements()[0].id;
    // Deleting the anchor row kills the anchor; the two rows below keep naming the image.
    s.feed_str(&cup(1, 0));
    s.feed_str("\x1b[M");
    assert_eq!(s.released(), [id]);
    assert_eq!(s.graphic(1, 0), Some(id));
    churn_links(&mut s, 4200);
    assert_eq!(s.graphic(1, 0), Some(id));
    assert_eq!(s.graphic(2, 0), Some(id));
}

#[test]
fn evicted_merged_entry_does_not_split_link() {
    let mut s = Session::new(80, 24);
    s.feed_str("\x1b]8;;http://keep.test\x07x\x1b]8;;\x07");
    s.feed_str(&cup(0, 0));
    s.image(10, 20, 1);
    let first = s.term.placements()[0].id;
    for index in 1..=256u16 {
        s.feed_str(&cup(index / 80 % 20 + 2, index % 80));
        s.feed(&dot());
    }
    churn_links(&mut s, 4200);
    s.term.snapshot_update(&mut s.state, s.now);
    let cell = s.state.rows()[0].cells[0];
    assert_eq!(cell.graphic, Some(first));
    let link = cell.hyperlink.expect("still linked");
    assert_eq!(
        &*s.state.hyperlink(link).expect("copied").uri,
        "http://keep.test"
    );
}

#[test]
fn every_live_placement_reachable_by_its_cells() {
    let mut s = Session::new(30, 12);
    for index in 0..6u16 {
        s.feed_str(&cup(index, index * 4));
        s.image(25, 45, 8);
    }
    s.feed_str("\x1b[2S\x1b[L");
    churn_links(&mut s, 4200);
    for placement in s.term.placements() {
        let pos = s
            .term
            .grid()
            .anchors()
            .get(placement.anchor)
            .expect("alive");
        assert_eq!(s.graphic_at(pos.row, pos.col), Some(placement.id));
    }
}

#[test]
fn trimmed_anchor_survivors_not_recycled() {
    let mut s = Session::with(
        10,
        5,
        Config {
            scrollback_limit: 2,
            ..Config::default()
        },
    );
    s.image(10, 60, 1);
    let id = s.term.placements()[0].id;
    s.feed_str(&cup(4, 0));
    s.feed_str("\n\n\n");
    assert_eq!(s.released(), [id], "the anchor row was trimmed");
    assert_eq!(s.graphic_at(RowId(1), 0), Some(id));
    churn_links(&mut s, 4200);
    assert_eq!(s.graphic_at(RowId(1), 0), Some(id));
    assert_eq!(s.graphic_at(RowId(2), 0), Some(id));
}

// ---- hand-off ----

#[test]
fn image_handed_out_once() {
    let mut s = Session::new(10, 5);
    s.image(10, 20, 1);
    let image = s.one_image();
    assert_eq!((image.width, image.height), (10, 20));
    assert_eq!(image.id, s.term.placements()[0].id);
    assert!(s.term.take_graphics().is_empty());
}

#[test]
fn ris_drops_pending_keeps_id_counter() {
    let mut s = Session::new(10, 5);
    s.image(10, 20, 1);
    s.feed_str("\x1bc");
    assert!(s.term.take_graphics().is_empty());
    s.image(10, 20, 1);
    assert_eq!(s.one_image().id, GraphicId(2));
}

#[test]
fn clear_screen_keeps_pending() {
    let mut s = Session::new(10, 5);
    s.image(10, 20, 1);
    s.feed_str("\x1b[2J\x1b[3J");
    assert_eq!(s.term.take_graphics().len(), 1);
}

#[test]
fn images_survive_suppressed_frame() {
    let mut s = Session::new(10, 5);
    let mut other = SnapshotState::new();
    s.term.snapshot_update(&mut s.state, s.now);
    s.term.snapshot_update(&mut other, s.now);
    s.feed_str("\x1b[?2026h");
    s.image(10, 20, 1);
    s.term.snapshot_update(&mut s.state, s.now);
    s.term.snapshot_update(&mut other, s.now);
    assert_eq!(s.state.placements().len(), 1);
    assert_eq!(other.placements().len(), 1);
    assert_eq!(s.term.take_graphics().len(), 1);
}

#[test]
fn aborted_dcs_stamps_no_cells() {
    let mut s = Session::new(10, 5);
    let stats = s.feed(b"\x1bPq~~~\x18");
    assert_eq!(stats.aborted_dcs, 1);
    assert!(s.term.take_graphics().is_empty());
    assert!(s.term.placements().is_empty());
    assert_eq!(s.cursor(), (0, 0));
    assert_eq!(s.graphic(0, 0), None);
}

#[test]
fn payload_cap_aborts_endless_sixel() {
    let mut s = Session::new(10, 5);
    s.feed(b"\x1bPq");
    let chunk = vec![b'?'; 1 << 20];
    let mut aborted = 0;
    for _ in 0..32 {
        aborted += s.feed(&chunk).aborted_dcs;
    }
    assert_eq!(aborted, 1);
    assert!(s.term.take_graphics().is_empty());
    assert!(s.term.placements().is_empty());
    assert_eq!(s.cursor(), (0, 0));
}

#[test]
fn corrupt_band_does_not_panic() {
    let mut s = Session::new(10, 5);
    s.feed(b"\x1bPq#0;2;100;0;0~~~~~!~~~#;;;;;;#999999999999~~\x01\x02-~-~\"9;9;9~\x1b\\");
    assert!(s.term.take_graphics().len() <= 1);
    s.feed_str("ok");
    assert!(s.row_text(s.cursor().0).contains("ok"));
}

#[test]
fn random_bytes_in_a_sixel_leave_the_terminal_usable() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut s = Session::new(40, 10);
    let mut bytes = b"\x1bPq".to_vec();
    bytes.extend((0..20_000).map(|_| 0x20 + (next() % 0x5F) as u8));
    bytes.extend_from_slice(b"\x1b\\");
    s.feed(&bytes);
    for image in s.term.take_graphics() {
        assert!(image.width <= 4096 && image.height <= 4096);
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );
    }
    s.feed_str("\x1bcplain");
    assert_eq!(s.row_text(0).trim_end(), "plain");
}

// ---- D2: the pixel queue ----

/// Three raster-only 4096 x 4096 images (64 MiB each) at rows 0, 1 and 2, none taken.
fn overfill_queue() -> Session {
    let mut s = Session::new(80, 24);
    for row in 0..3 {
        s.feed_str(&cup(row, 0));
        s.feed(&sixel("\"1;1;4096;4096"));
        if row < 2 {
            assert_eq!(s.term.stats().graphics_dropped, 0);
        }
    }
    s
}

#[test]
fn pending_cap_drops_oldest() {
    let mut s = overfill_queue();
    assert_eq!(s.term.stats().graphics_dropped, 1);
    assert_eq!(s.released(), [GraphicId(1)]);
    let taken = s.term.take_graphics();
    assert_eq!(
        taken.iter().map(|image| image.id).collect::<Vec<_>>(),
        [GraphicId(2), GraphicId(3)]
    );
    assert_eq!(s.term.placements().len(), 2);
}

#[test]
fn graphics_dropped_counts_per_feed() {
    let mut s = overfill_queue();
    assert_eq!(s.term.stats().graphics_dropped, 1);
    let stats = s.feed(b"x");
    assert_eq!(stats.graphics_dropped, 0);
}

// ---- DCS routing ----

#[test]
fn intermediate_dcs_q_never_decodes() {
    let mut s = Session::new(10, 5);
    let stats = s.feed(b"\x1bP$qm\x1b\\");
    assert_eq!(stats.unhandled_sequences, 0);
    assert!(s.replies().contains("1$r"));
    let stats = s.feed(b"\x1bP+q544e\x1b\\");
    assert_eq!(stats.unhandled_sequences, 0);
    assert!(s.term.take_graphics().is_empty());
    assert!(s.term.placements().is_empty());
    assert_eq!(s.graphic(0, 0), None);
}

#[test]
fn bare_dcs_q_places_one() {
    let mut s = Session::new(10, 5);
    let stats = s.feed(&dot());
    assert_eq!(stats.unhandled_sequences, 0);
    assert_eq!(s.term.placements().len(), 1);
    assert_eq!(s.one_image().rgba.len(), 4);
}

#[test]
fn parameterised_dcs_q_decodes() {
    let mut s = Session::new(10, 5);
    let stats = s.feed(b"\x1bP0;1;0q\"1;1;1;1~\x1b\\");
    assert_eq!(stats.unhandled_sequences, 0);
    let stats = s.feed(b"\x1bP9q\"1;1;1;1~\x1b\\");
    assert_eq!(stats.unhandled_sequences, 0);
    assert_eq!(s.term.take_graphics().len(), 2);
}

#[test]
fn query_after_empty_unterminated_sixel() {
    let mut s = Session::new(10, 5);
    let stats = s.feed(b"\x1bPq\x1bP$qm\x1b\\");
    assert_eq!(stats.unhandled_sequences, 0);
    assert!(s.replies().contains("1$r"));
    assert!(s.term.take_graphics().is_empty());
    assert!(s.term.placements().is_empty());
}

#[test]
fn query_after_nonempty_unterminated_sixel() {
    let mut s = Session::new(10, 5);
    let stats = s.feed(b"\x1bPq~\x1bP$qm\x1b\\");
    assert_eq!(stats.aborted_dcs, 0);
    assert_eq!(s.term.placements().len(), 1);
    assert_eq!(s.term.take_graphics().len(), 1);
    assert!(s.replies().contains("1$r"));
}

#[test]
fn one_mib_query_payload_buffers_nothing() {
    let mut s = Session::new(10, 5);
    let mut bytes = b"\x1bP$q".to_vec();
    bytes.extend(std::iter::repeat_n(b'm', 1 << 20));
    bytes.extend_from_slice(b"\x1b\\");
    let stats = s.feed(&bytes);
    assert_eq!(stats.unhandled_sequences, 1);
    assert!(s.term.take_graphics().is_empty());
    assert!(s.replies().is_empty());
}

#[test]
fn overflowed_intermediates_not_sixel() {
    let mut s = Session::new(10, 5);
    let stats = s.feed(b"\x1bP$+!q~~~\x1b\\");
    assert_eq!(stats.unhandled_sequences, 1);
    assert!(s.term.take_graphics().is_empty());
    assert!(s.term.placements().is_empty());
}

// ---- D4 and DA1 ----

#[test]
fn csi_14_t_reports_effective_cell() {
    let mut s = Session::new(80, 24);
    s.feed_str("\x1b[14t");
    assert_eq!(s.replies(), "\x1b[4;480;800t");
    s.term.set_cell_pixels(9, 18);
    s.feed_str("\x1b[14t");
    assert_eq!(s.replies(), "\x1b[4;432;720t");
    s.term.set_cell_pixels(9, 0);
    s.feed_str("\x1b[14t");
    assert_eq!(s.replies(), "\x1b[4;480;800t");
}

#[test]
fn da1_advertises_sixel() {
    let mut s = Session::new(80, 24);
    s.feed_str("\x1b[c");
    assert_eq!(s.replies(), "\x1b[?62;4;22c");
}

// ---- reclamation ----

#[test]
fn ten_thousand_images_keep_extras_bounded() {
    let mut s = Session::with(
        20,
        5,
        Config {
            scrollback_limit: 0,
            ..Config::default()
        },
    );
    let unit = [dot().as_slice(), b"\r\n"].concat();
    let mut stream = Vec::new();
    for _ in 0..10_000 {
        stream.extend_from_slice(&unit);
    }
    s.feed(&stream);
    assert!(
        s.term.interner().extras.entries() <= 4_200,
        "{}",
        s.term.interner().extras.entries()
    );
    assert!(s.term.placements().len() <= MAX_PLACEMENTS);
    let last = s.term.placements().last().expect("a live placement").id;
    let pos = s.anchor(s.term.placements().len() - 1).expect("alive");
    assert_eq!(s.graphic_at(pos.row, pos.col), Some(last));
    // The queue stays bounded too once drained.
    assert!(s.term.take_graphics().len() <= 10_000);
}

#[test]
fn graphic_state_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Arc<GraphicData>>();
    assert_send_sync::<Placement>();
}

#[test]
fn virtual_cell_is_ten_by_twenty() {
    assert_eq!(VIRTUAL_CELL, (10, 20));
}

#[test]
fn renderer_finds_offsets_through_the_snapshot() {
    let mut s = Session::new(20, 10);
    s.feed_str(&cup(2, 3));
    s.image(25, 45, 8);
    let id = s.term.placements()[0].id;
    assert_eq!(s.offset(2, 3), Some((id, 0, 0)));
    assert_eq!(s.offset(4, 5), Some((id, 2, 2)));
    assert_eq!(s.offset(3, 4), Some((id, 1, 1)));
    assert_eq!(s.offset(1, 3), None);
    assert_eq!(s.offset(2, 6), None);
}

#[test]
fn chunking_does_not_change_the_image() {
    let stream = [b"ab".as_slice(), &image(25, 45, 8), b"cd"].concat();
    let mut whole = Session::new(20, 10);
    whole.feed(&stream);
    let mut bytewise = Session::new(20, 10);
    for byte in &stream {
        bytewise.feed(std::slice::from_ref(byte));
    }
    assert_eq!(whole.term.placements().len(), 1);
    assert_eq!(
        whole.term.placements()[0].cols,
        bytewise.term.placements()[0].cols
    );
    assert_eq!(
        whole.term.placements()[0].rows,
        bytewise.term.placements()[0].rows
    );
    assert_eq!(whole.cursor(), bytewise.cursor());
    let (a, b) = (whole.one_image(), bytewise.one_image());
    assert_eq!((a.width, a.height, &a.rgba), (b.width, b.height, &b.rgba));
}
