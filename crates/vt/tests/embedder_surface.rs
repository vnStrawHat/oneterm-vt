//! The surface an embedder with its own fixtures and sinks builds on, through public paths only.

use std::time::Instant;

use oneterm_vt::grid::TerminalGrid;
use oneterm_vt::parser::{Dispatch, Params, Parser};
use oneterm_vt::search::{GridText, SearchOptions, SearchPattern, search_grid_text};
use oneterm_vt::{
    Attrs, Cell, CellContent, CellWidth, ColorKey, Config, EventBatch, Extras, ExtrasId, GraphicId,
    Hyperlink, HyperlinkId, Interner, Size, SnapshotContent, SnapshotState, Style, Terminal,
    VtEvent,
};

fn terminal(cols: u16, rows: u16) -> Terminal {
    Terminal::new(Size { rows, cols }, Config::default())
}

#[test]
fn s1_root_names_compile() {
    let _: (Option<Extras>, Option<Hyperlink>, Option<Interner>) = (None, None, None);
    let _: Option<HyperlinkId> = None;
    assert_eq!(ExtrasId::NONE, ExtrasId(0));
    assert_eq!(
        VtEvent::GraphicReleased(GraphicId(1)),
        VtEvent::GraphicReleased(GraphicId(1))
    );
}

#[test]
fn s2_signatures_are_pinned() {
    let _: fn(&mut Terminal) -> &mut TerminalGrid = Terminal::grid_mut;
    let _: fn(&mut Terminal) -> &mut Interner = Terminal::interner_mut;
}

#[test]
fn s3_fixture_writer_reads_back_through_the_snapshot() {
    let mut term = terminal(6, 3);
    let (mut state, now) = (SnapshotState::new(), Instant::now());
    term.snapshot_update(&mut state, now);

    let interner = term.interner_mut();
    let bold = Style {
        attrs: Attrs::BOLD,
        ..Style::DEFAULT
    };
    let style = interner.style(bold); // by value: Style is Copy
    let cluster = interner.grapheme("e\u{301}");
    let link = interner.hyperlinks.intern(None, "https://example.com");
    let extras = interner.extras(Extras {
        hyperlink: link,
        graphic: None,
    });
    let wide = Cell::EMPTY
        .with_content(CellContent::Scalar('\u{4e2d}'))
        .with_width(CellWidth::Wide);
    let grapheme = Cell::EMPTY.with_content(CellContent::Grapheme(cluster));
    let linked = Cell::EMPTY
        .with_content(CellContent::Scalar('l'))
        .with_style(style)
        .with_style(style)
        .with_extras(extras);

    let id = term.screen().screen_top();
    term.grid_mut().begin_batch();
    let mut row = term.grid_mut().screen_mut().row_mut(id).unwrap();
    row.set(0, wide);
    row.set(2, grapheme);
    row.set(3, linked);
    row.set_wrapped(true);
    term.snapshot_update(&mut state, now);

    let row = &state.rows()[0];
    assert!(row.wrapped);
    assert_eq!(row.cells[0].content, SnapshotContent::Scalar('\u{4e2d}'));
    assert_eq!(row.cells[0].width, CellWidth::Wide);
    assert!(row.cells[1].width.is_spacer());
    let SnapshotContent::Cluster { start, len } = row.cells[2].content else {
        panic!("expected a cluster");
    };
    assert_eq!(row.cluster(start, len), ['e', '\u{301}']);
    assert_eq!(*row.style_of(&row.cells[3]), bold);
    let link_id = row.cells[3].hyperlink.unwrap();
    assert_eq!(
        &*state.hyperlink(link_id).unwrap().uri,
        "https://example.com"
    );
    let resolved = term.interner().hyperlinks.resolve(link_id).unwrap();
    assert_eq!(&*resolved.uri, "https://example.com");
}

/// What an embedder's theme answers: no wildcard arm, so a new key breaks this build.
fn theme_slot(key: ColorKey) -> u8 {
    match key {
        ColorKey::Palette(_) => 0,
        ColorKey::Foreground => 1,
        ColorKey::Background => 2,
        ColorKey::Cursor => 3,
        ColorKey::SelectionBackground => 4,
        ColorKey::SelectionForeground => 5,
    }
}

#[test]
fn s4_color_key_matches_exhaustively() {
    assert_eq!(theme_slot(ColorKey::Cursor), 3);
}

#[derive(Default)]
struct Sink {
    seen: Vec<(char, Vec<u8>, bool, u8)>,
}

impl Dispatch for Sink {
    fn print_str(&mut self, _text: &str) {}
    fn execute(&mut self, _byte: u8) {}
    fn esc(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        self.seen.push(('e', intermediates.to_vec(), ignore, byte));
    }
    fn csi(&mut self, _params: &Params, intermediates: &[u8], ignore: bool, byte: u8) {
        let marker = intermediates.first().copied().unwrap_or(0);
        self.seen.push(('c', vec![marker], ignore, byte));
    }
    fn dcs_hook(&mut self, _params: &Params, intermediates: &[u8], ignore: bool, byte: u8) {
        self.seen.push(('d', intermediates.to_vec(), ignore, byte));
    }
}

#[test]
fn s5_dispatch_sink_takes_slices() {
    let mut sink = Sink::default();
    Parser::new().advance(&mut sink, b"\x1b(0\x1b[?25h\x1bP$qm\x1b\x5c");
    assert_eq!(
        sink.seen,
        [
            ('e', b"(".to_vec(), false, b'0'),
            ('c', b"?".to_vec(), false, b'h'),
            ('d', b"$".to_vec(), false, b'q'),
            ('e', Vec::new(), false, 92), // the ST that ends the DCS
        ]
    );
}

#[test]
fn s6_viewport_by_hand() {
    let mut term = terminal(10, 3);
    let (mut batch, now) = (EventBatch::new(), Instant::now());
    term.feed(b"1\r\n2\r\n3\r\n4\r\n5\r\n6", &mut batch, now);
    let top = term.viewport().top;
    term.grid_mut().screen_mut().scroll_viewport(-2);
    term.grid_mut().sync_anchors();
    assert_eq!(term.viewport().top, top - 2);

    let mut state = SnapshotState::new();
    term.snapshot_update(&mut state, now);
    assert_eq!(state.viewport_top(), top - 2);
    assert_eq!(state.scroll_offset(), 2);

    let newest = term.screen().newest();
    assert!(term.grid_mut().screen_mut().row_mut(newest + 1).is_none());
}

#[test]
fn s7_search_match_spans_wrapped_rows() {
    let mut term = terminal(10, 3);
    term.feed(b"0123456789abc", &mut EventBatch::new(), Instant::now());
    let text = GridText::from_terminal(&term);
    let found = search_grid_text(
        &text,
        SearchPattern::Literal("89ab"),
        SearchOptions::default(),
    );
    assert_eq!(found[0].end_row, found[0].row + 1);
    assert_eq!((found[0].start_col, found[0].end_col), (8, 2));
}
