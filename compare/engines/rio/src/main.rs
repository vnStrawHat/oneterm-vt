//! rio-vt 0.5.28 (no PTY, no graphics feature) behind the comparison harness.

use std::process::ExitCode;

use compare_harness::{
    Capability, Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells,
};
use rio_vt::ansi::CursorShape;
use rio_vt::config::colors::{AnsiColor, NamedColor};
use rio_vt::crosswords::pos::{Column, Line, Pos};
use rio_vt::crosswords::square::Wide;
use rio_vt::crosswords::style::StyleFlags;
use rio_vt::crosswords::{Crosswords, CrosswordsSize};
use rio_vt::event::{VoidListener, WindowId};
use rio_vt::performer::handler::Processor;

struct Rio {
    term: Crosswords<VoidListener>,
    processor: Processor,
    cols: usize,
    rows: usize,
}

fn color(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Indexed(index) => Color::Indexed(index),
        AnsiColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        // Black..LightWhite are palette 0-15; foreground, background, cursor and the dim
        // slots are the default colour.
        AnsiColor::Named(named) => match named as usize {
            slot @ 0..16 => Color::Indexed(slot as u8),
            _ => Color::Default,
        },
    }
}

impl Rio {
    fn push_line(&self, line: Line, out: &mut RowCells) {
        let grid = &self.term.grid;
        for column in 0..self.cols {
            let pos = Pos {
                row: line,
                col: Column(column),
            };
            let square = grid[pos];
            let wide = square.wide();
            if wide == Wide::Spacer {
                out.cell(Cell {
                    spacer: true,
                    ..Cell::default()
                });
                continue;
            }
            let text: String = if wide == Wide::LeadingSpacer {
                String::new()
            } else {
                grid.cell_text(pos)
                    .filter(|c| *c != ' ' && *c != '\0')
                    .collect()
            };
            let style = grid.style_of(&square);
            let uri = if square.has_hyperlink() {
                square
                    .extras_id_checked()
                    .and_then(|id| grid.extras_table.get(id))
                    .and_then(|extras| extras.hyperlink.as_ref())
                    .map(|link| link.uri())
            } else {
                None
            };
            out.cell(Cell {
                text: &text,
                wide: wide == Wide::Wide,
                spacer: false,
                fg: color(style.fg),
                bg: color(style.bg),
                flags: Flags {
                    bold: style.flags.contains(StyleFlags::BOLD),
                    italic: style.flags.contains(StyleFlags::ITALIC),
                    underline: style.flags.intersects(StyleFlags::ALL_UNDERLINES),
                    inverse: style.flags.contains(StyleFlags::INVERSE),
                },
                uri,
            });
        }
    }
}

impl Engine for Rio {
    const INFO: EngineInfo = EngineInfo {
        name: "rio",
        version: "0.5.28",
        source: "crates.io rio-vt =0.5.28, default-features = false",
        language: "rust",
        build: "release, thin LTO, no PTY",
        capabilities: &[
            Capability::Sixel,
            Capability::Graphemes,
            Capability::Hyperlinks,
        ],
        labels: &[],
    };

    fn new(geometry: Geometry) -> Self {
        let (cols, rows) = (usize::from(geometry.cols), usize::from(geometry.rows));
        Self {
            term: Crosswords::new(
                CrosswordsSize::new(cols, rows),
                CursorShape::Block,
                VoidListener,
                WindowId::from(0),
                0,
                geometry.scrollback,
            ),
            processor: Processor::default(),
            cols,
            rows,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    fn finish(&mut self) {
        // Like vte, rio buffers a `?2026` window until it closes, times out or is flushed.
        if self.processor.sync_bytes_count() > 0 {
            self.processor.stop_sync(&mut self.term);
        }
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let cursor = self.term.grid.cursor.pos;
        out.cursor(usize::try_from(cursor.row.0).unwrap_or(0), cursor.col.0);
        if out.wants_history() {
            let history = self.term.history_size().min(self.rows);
            for k in (1..=history).rev() {
                let line = Line(-i32::try_from(k).unwrap_or(0));
                out.history_row(|row| self.push_line(line, row));
            }
        }
        for line in 0..self.rows {
            let line = Line(i32::try_from(line).unwrap_or(0));
            out.row(|row| self.push_line(line, row));
        }
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Rio>()
}

// Keeps `NamedColor` linked to the match in `color`, which names its discriminants by value.
const _: () = assert!(NamedColor::Black as usize == 0);
