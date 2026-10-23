//! One test per `FeedStats` field, through the public API only.

use std::time::Instant;

use oneterm_vt::parser::DCS_MAX_BYTES;
use oneterm_vt::{Config, EventBatch, FeedStats, Size, Terminal};

fn terminal(cols: u16, rows: u16) -> Terminal {
    Terminal::new(Size { rows, cols }, Config::default())
}

fn feed(term: &mut Terminal, batch: &mut EventBatch, bytes: &[u8]) -> FeedStats {
    term.feed(bytes, batch, Instant::now())
}

/// Feeds `bytes`, then checks that counters are per call: a following plain feed reads zeros.
fn stats_of(term: &mut Terminal, bytes: &[u8]) -> FeedStats {
    let mut batch = EventBatch::new();
    let stats = feed(term, &mut batch, bytes);
    let mut quiet = FeedStats::default();
    quiet.bytes = 1;
    assert_eq!(feed(term, &mut batch, b"a"), quiet, "counters are per call");
    stats
}

#[test]
fn bytes() {
    let mut term = terminal(10, 3);
    assert_eq!(stats_of(&mut term, b"abc").bytes, 3);
    assert_eq!(term.stats().bytes, 1);
}

#[test]
fn rows_scrolled() {
    let mut term = terminal(10, 3);
    assert_eq!(stats_of(&mut term, b"\n\n\n\n\n").rows_scrolled, 3);
}

#[test]
fn unhandled_sequences() {
    let mut term = terminal(10, 3);
    assert_eq!(stats_of(&mut term, b"\x1b[?9999z").unhandled_sequences, 1);
}

#[test]
fn malformed_sequences() {
    let mut term = terminal(10, 3);
    let stats = stats_of(&mut term, b"\x1b]52;c;!!!!\x07");
    assert_eq!(stats.malformed_sequences, 1);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn truncated_osc() {
    let mut term = terminal(10, 3);
    let mut stream = b"\x1b]0;".to_vec();
    stream.extend(std::iter::repeat_n(b't', 3 * 1024));
    stream.push(0x07);
    assert_eq!(stats_of(&mut term, &stream).truncated_osc, 1);
}

#[test]
fn aborted_dcs() {
    let mut term = terminal(10, 3);
    let mut stream = b"\x1bP+q".to_vec();
    stream.extend(std::iter::repeat_n(b'4', DCS_MAX_BYTES + 1));
    stream.extend_from_slice(b"\x1b\\");
    let stats = stats_of(&mut term, &stream);
    assert_eq!(stats.aborted_dcs, 1);
    assert_eq!(stats.unhandled_sequences, 0);
}

#[test]
fn hyperlink_table_exhausted() {
    let mut term = terminal(160, 45);
    let mut stream = Vec::new();
    for link in 0..65_537u32 {
        stream.extend_from_slice(format!("\x1b]8;;http://l/{link}\x07x").as_bytes());
    }
    let stats = stats_of(&mut term, &stream);
    assert!(stats.hyperlink_table_exhausted >= 1);
}

#[test]
fn style_table_exhausted() {
    let mut term = terminal(160, 45);
    let mut stream = Vec::new();
    for color in 0..65_536u32 {
        let (g, b) = ((color >> 8) & 255, color & 255);
        stream.extend_from_slice(format!("\x1b[38;2;0;{g};{b}mx").as_bytes());
    }
    assert_eq!(stats_of(&mut term, &stream).style_table_exhausted, 1);
}

#[test]
fn grapheme_truncated() {
    let mut term = terminal(10, 3);
    let stream = format!("e{}", "\u{301}".repeat(16));
    assert_eq!(stats_of(&mut term, stream.as_bytes()).grapheme_truncated, 1);
}

#[test]
fn grapheme_table_exhausted() {
    let config = Config {
        scrollback_limit: 10_000,
        ..Config::default()
    };
    let mut term = Terminal::new(
        Size {
            rows: 45,
            cols: 160,
        },
        config,
    );
    let mut batch = EventBatch::new();
    let stream: String = (0..270_000u32)
        .map(|i| {
            let base = char::from_u32(0x4E00 + i % 20_000).unwrap_or('x');
            let mark = char::from_u32(0x300 + i / 20_000).unwrap_or('\u{301}');
            format!("{base}{mark}")
        })
        .collect();
    let mut exhausted = 0u32;
    for chunk in stream.as_bytes().chunks(64 * 1024) {
        exhausted =
            exhausted.saturating_add(feed(&mut term, &mut batch, chunk).grapheme_table_exhausted);
    }
    assert!(exhausted > 0);
    assert!(term.interner().graphemes.entries() <= 1 << 18);
    assert_eq!(
        feed(&mut term, &mut batch, b"a").grapheme_table_exhausted,
        0
    );
}

#[test]
fn dropped_cluster_carries() {
    let mut term = terminal(10, 3);
    let stream = format!("\x1b[?2027he{}", "\u{301}".repeat(40));
    assert_eq!(
        stats_of(&mut term, stream.as_bytes()).dropped_cluster_carries,
        1
    );
}

#[test]
fn wide_glyphs_dropped() {
    let mut term = terminal(1, 3);
    assert_eq!(stats_of(&mut term, "中".as_bytes()).wide_glyphs_dropped, 1);
}
