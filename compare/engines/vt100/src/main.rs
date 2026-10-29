//! vt100 0.18.1 (vte 0.15.0) behind the comparison harness.
//!
//! vt100 has no public flush for a `?2026` window; the corpus closes every window it opens
//! (vt-bench test V9), so `finish` has nothing to do.

use std::num::NonZeroU16;
use std::process::ExitCode;

use compare_harness::{Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells};

struct Vt100 {
    parser: vt100::Parser,
    cols: u16,
    rows: u16,
}

fn color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn push_row(screen: &vt100::Screen, row: u16, cols: u16, out: &mut RowCells) {
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            out.cell(Cell::default());
            continue;
        };
        out.cell(Cell {
            text: cell.contents(),
            wide: cell.is_wide(),
            spacer: cell.is_wide_continuation(),
            fg: color(cell.fgcolor()),
            bg: color(cell.bgcolor()),
            flags: Flags {
                bold: cell.bold(),
                italic: cell.italic(),
                underline: cell.underline(),
                inverse: cell.inverse(),
            },
            uri: None,
        });
    }
}

impl Engine for Vt100 {
    const INFO: EngineInfo = EngineInfo {
        name: "vt100",
        version: "0.18.1",
        source: "crates.io vt100 =0.18.1 (vte 0.15.0)",
        language: "rust",
        build: "release, thin LTO",
        capabilities: &[],
        labels: &[],
    };

    fn new(geometry: Geometry) -> Self {
        let size = |n: u16| NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN);
        Self {
            parser: vt100::Parser::new(
                size(geometry.rows),
                size(geometry.cols),
                geometry.scrollback,
            ),
            cols: geometry.cols,
            rows: geometry.rows,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        out.cursor(usize::from(row), usize::from(col));
        if out.wants_history() {
            // Scrolled back by `h` rows, the first `h` visible rows are the newest history.
            // `set_scrollback` needs `&mut`, so the one final dump works on a copy.
            let history = u16::try_from(screen.scrollback_len())
                .unwrap_or(u16::MAX)
                .min(self.rows);
            let mut scrolled = screen.clone();
            scrolled.set_scrollback(usize::from(history));
            for row in 0..history {
                out.history_row(|cells| push_row(&scrolled, row, self.cols, cells));
            }
        }
        for row in 0..self.rows {
            out.row(|cells| push_row(screen, row, self.cols, cells));
        }
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Vt100>()
}
