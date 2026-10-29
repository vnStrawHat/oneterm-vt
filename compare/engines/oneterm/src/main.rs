//! oneterm-vt behind the comparison harness: `Terminal::feed` plus the image drain an
//! embedder performs after every chunk (as vt-bench tier 2 does).

use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

use compare_harness::{
    Capability, Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells,
};
use oneterm_vt::grid::Screen;
use oneterm_vt::{
    Attrs, CellContent, CellWidth, Config, EventBatch, NamedColor, RowId, Size, Terminal,
};

struct Oneterm {
    term: Terminal,
    batch: EventBatch,
    now: Instant,
}

fn color(color: oneterm_vt::Color, default: NamedColor) -> Color {
    match color {
        oneterm_vt::Color::Palette(index) => Color::Indexed(index),
        oneterm_vt::Color::Rgb(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        oneterm_vt::Color::Named(named) if named == default => Color::Default,
        // Black..BrightWhite are palette 0-15; the other slots (cursor, bright default) are not
        // something SGR can set on a cell.
        oneterm_vt::Color::Named(named) => match named as u8 {
            slot @ 0..16 => Color::Indexed(slot),
            _ => Color::Default,
        },
    }
}

impl Oneterm {
    fn push_row(&self, screen: &Screen, row: RowId, out: &mut RowCells) {
        let interner = self.term.interner();
        let mut utf8 = [0; 4];
        for cell in screen.row(row).cells() {
            let style = interner.resolve_style(cell.style_id());
            let text = match cell.content() {
                CellContent::Scalar(' ') => "",
                CellContent::Scalar(c) => c.encode_utf8(&mut utf8),
                CellContent::Grapheme(id) => interner.resolve_grapheme(id),
            };
            let width = cell.width();
            let uri = interner
                .resolve_extras(cell.extras_id())
                .hyperlink
                .and_then(|id| interner.hyperlinks.resolve(id))
                .map(|link| &*link.uri);
            out.cell(Cell {
                text,
                wide: width == CellWidth::Wide,
                spacer: width == CellWidth::WideSpacer,
                fg: color(style.fg, NamedColor::Foreground),
                bg: color(style.bg, NamedColor::Background),
                flags: Flags {
                    bold: style.attrs.contains(Attrs::BOLD),
                    italic: style.attrs.contains(Attrs::ITALIC),
                    underline: style.attrs.intersects(Attrs::ALL_UNDERLINES),
                    inverse: style.attrs.contains(Attrs::INVERSE),
                },
                uri,
            });
        }
    }
}

impl Engine for Oneterm {
    const INFO: EngineInfo = EngineInfo {
        name: "oneterm",
        version: env!("ONETERM_VT_VERSION"),
        source: "this repository, crates/vt",
        language: "rust",
        build: "release, thin LTO, default-features = false",
        capabilities: &[
            Capability::Sixel,
            Capability::Graphemes,
            Capability::Hyperlinks,
        ],
        labels: &[],
    };

    fn new(geometry: Geometry) -> Self {
        let config = Config {
            scrollback_limit: u32::try_from(geometry.scrollback).unwrap_or(u32::MAX),
            ..Config::default()
        };
        Self {
            term: Terminal::new(
                Size {
                    rows: geometry.rows,
                    cols: geometry.cols,
                },
                config,
            ),
            batch: EventBatch::new(),
            now: Instant::now(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.term.feed(bytes, &mut self.batch, self.now);
        // An embedder drains the images after every chunk; an empty-Vec swap when there are none.
        black_box(self.term.take_graphics());
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let screen = self.term.screen();
        let rows = u64::from(screen.rows());
        out.cursor(
            usize::from(screen.cursor_row_index()),
            usize::from(screen.cursor().pos.col),
        );
        if out.wants_history() {
            let primary = self.term.grid().primary();
            let count = u64::from(primary.history_len()).min(rows);
            let primary_top = primary.screen_top();
            for n in 0..count {
                let id = primary_top - (count - n);
                out.history_row(|row| self.push_row(primary, id, row));
            }
        }
        let top = screen.screen_top();
        for n in 0..rows {
            out.row(|row| self.push_row(screen, top + n, row));
        }
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Oneterm>()
}
