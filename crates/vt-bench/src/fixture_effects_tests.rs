//! What the newer fixtures do to a whole terminal, not just to the parser.

use std::sync::Arc;
use std::time::Instant;

use oneterm_vt::{
    Cell, CellContent, Config, EventBatch, ExtrasId, GraphicData, Mode, Size, Terminal,
};

use super::*;
use crate::throughput::CHUNK;

/// A terminal fed one fixture, and what an embedder would have drained along the way.
struct Fed {
    term: Terminal,
    images: Vec<Arc<GraphicData>>,
    /// Every sequence or item the engine counted as unhandled, truncated, aborted, dropped or
    /// degraded, summed over all feeds.
    refused: u32,
    /// Whether the alternate screen was shown after any feed.
    entered_alt_screen: bool,
}

fn feed_fixture(name: &str, target: usize, chunk: usize) -> (Fed, Vec<u8>) {
    let fixture = FIXTURES
        .iter()
        .chain(&COMPARE_FIXTURES)
        .find(|f| f.name == name)
        .expect("fixture exists");
    let bytes = (fixture.make)(target);
    let size = Size {
        rows: LINES as u16,
        cols: COLUMNS as u16,
    };
    let mut term = Terminal::new(size, Config::default());
    let mut batch = EventBatch::new();
    let (mut images, mut refused, mut entered_alt_screen) = (Vec::new(), 0, false);
    for part in bytes.chunks(chunk) {
        let stats = term.feed(part, &mut batch, Instant::now());
        refused += [
            stats.unhandled_sequences,
            stats.malformed_sequences,
            stats.truncated_osc,
            stats.aborted_dcs,
            stats.hyperlink_table_exhausted,
            stats.style_table_exhausted,
            stats.grapheme_truncated,
            stats.grapheme_table_exhausted,
            stats.dropped_cluster_carries,
            stats.wide_glyphs_dropped,
            stats.graphics_dropped,
        ]
        .iter()
        .sum::<u32>();
        images.extend(term.take_graphics());
        entered_alt_screen |= term.mode(Mode::AltScreen);
    }
    let fed = Fed {
        term,
        images,
        refused,
        entered_alt_screen,
    };
    (fed, bytes)
}

/// The cells of the visible rows.
fn visible_cells(term: &Terminal) -> Vec<Cell> {
    let view = term.viewport();
    (0..u64::from(view.rows))
        .flat_map(|n| term.screen().row(view.top + n).cells().to_vec())
        .collect()
}

#[test]
fn grapheme_clusters_form_multi_scalar_cells() {
    let (fed, _) = feed_fixture("grapheme_clusters", 1 << 16, CHUNK);
    let term = &fed.term;
    assert!(term.mode(Mode::GraphemeClusters));
    assert_eq!(fed.refused, 0);
    let scalars: Vec<usize> = visible_cells(term)
        .into_iter()
        .filter_map(|cell| match cell.content() {
            CellContent::Grapheme(id) => Some(term.interner().resolve_grapheme(id).chars().count()),
            CellContent::Scalar(_) => None,
        })
        .collect();
    assert!(scalars.len() > 1000, "{} cluster cells", scalars.len());
    assert!(scalars.iter().all(|count| *count > 1));
    assert_eq!(scalars.iter().max(), Some(&7), "the family sequence");
}

#[test]
fn hyperlinks_resolve_and_the_last_one_is_closed() {
    let (fed, _) = feed_fixture("hyperlinks", 1 << 16, CHUNK);
    let term = &fed.term;
    let links = &term.interner().hyperlinks;
    assert_eq!(fed.refused, 0);
    assert!(links.len() > 50, "{} links", links.len());

    let uris: Vec<_> = visible_cells(term)
        .into_iter()
        .filter_map(|cell| term.interner().resolve_extras(cell.extras_id()).hyperlink)
        .map(|id| links.resolve(id).expect("a cell names a stored link"))
        .collect();
    assert!(uris.len() > 500, "{} linked cells", uris.len());
    assert!(
        uris.iter()
            .all(|link| (200..=1000).contains(&link.uri.len()))
    );
    assert!(uris.iter().any(|link| link.implicit));
    assert!(uris.iter().any(|link| !link.implicit));
    assert!(
        uris.len() < visible_cells(term).len() / 2,
        "text outside links"
    );
    let pen = term.screen().cursor().template();
    assert_eq!(pen.extras_id(), ExtrasId::NONE);
}

#[test]
fn mode_churn_ends_where_it_started() {
    // Small chunks, so the alternate screen is caught while it is shown.
    let (fed, _) = feed_fixture("mode_churn", 1 << 18, 64);
    let term = &fed.term;
    assert!(fed.entered_alt_screen);
    assert_eq!(fed.refused, 0);
    for off in [
        Mode::AltScreen,
        Mode::BracketedPaste,
        Mode::MouseClick,
        Mode::SgrMouse,
        Mode::SyncUpdate,
    ] {
        assert!(!term.mode(off), "{off:?}");
    }
    for on in [Mode::ShowCursor, Mode::LineWrap] {
        assert!(term.mode(on), "{on:?}");
    }
    assert_eq!(term.title(), Some("shell"));
    assert!(term.lines_produced() > 0, "shell lines reach the history");
}

#[test]
fn sixel_large_decodes_every_image_at_full_size() {
    let (fed, bytes) = feed_fixture("sixel_large", 1 << 19, CHUNK);
    assert_eq!(fed.refused, 0);
    let starts = bytes.windows(3).filter(|w| w == b"\x1bPq").count();
    assert!(starts >= 3, "{starts} images");
    assert_eq!(fed.images.len(), starts);
    let sizes: Vec<(u32, u32)> = fed.images.iter().map(|g| (g.width, g.height)).collect();
    assert_eq!(sizes[..3], [(1024, 768), (800, 600), (640, 480)]);
    for image in &fed.images {
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );
        let painted = image.rgba.chunks_exact(4).filter(|px| px[3] == 255).count();
        assert!(painted * 2 > image.rgba.len() / 4, "image {:?}", image.id);
    }
}

/// The visible rows as text, trailing blanks trimmed.
fn visible_rows(term: &Terminal) -> Vec<String> {
    let top = term.screen().visible_top();
    (0..u64::from(term.size().rows))
        .map(|n| term.row_text(top + n).trim_end().to_string())
        .collect()
}

#[test]
fn light_cells_leaves_the_alt_screen_full_of_the_last_letter() {
    let (fed, bytes) = feed_fixture("light_cells", 1 << 16, CHUNK);
    let term = &fed.term;
    assert!(term.mode(Mode::AltScreen));
    assert_eq!(fed.refused, 0);
    let last = char::from(*bytes.last().expect("a non-empty stream"));
    let cells = visible_cells(term);
    assert_eq!(cells.len(), COLUMNS * LINES);
    assert!(
        cells
            .iter()
            .all(|cell| matches!(cell.content(), CellContent::Scalar(c) if c == last))
    );
}

#[test]
fn scroll_region_small_keeps_the_rows_below_the_region_blank() {
    let (fed, _) = feed_fixture("scroll_region_small", 1 << 14, CHUNK);
    assert_eq!(fed.refused, 0);
    let rows = visible_rows(&fed.term);
    assert!(rows[..21].iter().all(|row| row == "y"));
    assert!(rows[22..].iter().all(String::is_empty), "{rows:?}");
}

#[test]
fn long_osc_leaves_the_status_text_and_no_title() {
    let (fed, bytes) = feed_fixture("long_osc", 1 << 17, CHUNK);
    assert_eq!(fed.term.title(), None);
    let units = bytes.windows(3).filter(|w| w == b"\x1b]6").count();
    let rows = visible_rows(&fed.term);
    let last = rows.iter().rposition(|row| !row.is_empty()).expect("text");
    assert_eq!(rows[last], format!("osc {}", units - 1));
}

#[test]
fn unique_styles_keep_a_style_per_cell() {
    // 4 MiB is about 250,000 cells, each with a colour of its own, all still in the history:
    // far past what 16-bit style ids could name, and within the table of a million.
    let (fed, _) = feed_fixture("unique_styles", 1 << 22, CHUNK);
    assert!(fed.term.interner().styles.entries() > 200_000);
    assert_eq!(fed.refused, 0);
}

#[test]
fn kitty_images_are_placed_three_times_and_handed_out_once() {
    // 1 MiB: a cycle of three images is about 111 kB.
    let (fed, bytes) = feed_fixture("kitty_images", 1 << 20, CHUNK);
    let units = bytes.windows(6).filter(|w| w == b"_Ga=T,").count();
    assert!(units >= 24, "{units} images");
    assert_eq!(fed.refused, 0);
    assert_eq!(
        fed.images.len(),
        units,
        "one image per unit, however often it is placed"
    );
    // The newest unit: its image and three placements of it.
    let placements = fed.term.placements();
    let newest = placements.last().expect("placements");
    assert_eq!(
        placements
            .iter()
            .filter(|p| p.image == newest.image)
            .count(),
        3
    );
    assert_eq!((newest.cols, newest.rows), (10, 4));
    assert_eq!(fed.images.last().map(|image| image.id), Some(newest.image));
}

#[test]
fn unique_links_are_swept_instead_of_refused() {
    // 8 MiB is about 140,000 links against a table of 65,536; only 10,045 rows are live.
    let (fed, _) = feed_fixture("unique_links", 1 << 23, CHUNK);
    assert_eq!(fed.refused, 0);
    assert!(fed.term.interner().hyperlinks.len() <= 1 << 16);
}
