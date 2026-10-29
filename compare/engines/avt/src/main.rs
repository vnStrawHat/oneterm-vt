//! avt 0.19.0 behind the comparison harness.
//!
//! avt takes `&str` only, so the adapter decodes UTF-8 inside the timer (label
//! `+utf8 decode`); every other engine decodes inside its own feed call.

mod utf8;

use std::process::ExitCode;

use avt::{Line, Terminal};
use compare_harness::{Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells};

use crate::utf8::Utf8Feeder;

struct Avt {
    terminal: Terminal,
    decoder: Utf8Feeder,
    rows: usize,
}

fn color(color: Option<avt::Color>) -> Color {
    match color {
        None => Color::Default,
        Some(avt::Color::Indexed(index)) => Color::Indexed(index),
        Some(avt::Color::RGB(rgb)) => Color::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

fn push_line(line: &Line, out: &mut RowCells) {
    for cell in line.cells() {
        let pen = cell.pen();
        let mut utf8 = [0; 4];
        let text: &str = if cell.char() == ' ' {
            ""
        } else {
            cell.char().encode_utf8(&mut utf8)
        };
        out.cell(Cell {
            text,
            wide: cell.width() == 2,
            spacer: cell.width() == 0,
            fg: color(pen.foreground()),
            bg: color(pen.background()),
            flags: Flags {
                bold: pen.is_bold(),
                italic: pen.is_italic(),
                underline: pen.is_underline(),
                inverse: pen.is_inverse(),
            },
            uri: None,
        });
    }
}

impl Engine for Avt {
    const INFO: EngineInfo = EngineInfo {
        name: "avt",
        version: "0.19.0",
        source: "crates.io avt =0.19.0",
        language: "rust",
        build: "release, thin LTO",
        capabilities: &[],
        labels: &["+utf8 decode"],
    };

    fn new(geometry: Geometry) -> Self {
        let Ok(terminal) = Terminal::with_scrollback(
            usize::from(geometry.cols),
            usize::from(geometry.rows),
            Some(geometry.scrollback),
        ) else {
            // The harness never asks for a zero-sized grid.
            eprintln!("engine-avt: the grid must be at least 1x1");
            std::process::exit(2);
        };
        Self {
            terminal,
            decoder: Utf8Feeder::default(),
            rows: usize::from(geometry.rows),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        let terminal = &mut self.terminal;
        self.decoder.push(bytes, &mut |text| terminal.feed(text));
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let cursor = self.terminal.cursor();
        out.cursor(cursor.row, cursor.col);
        if out.wants_history() {
            // `all_lines` is scrollback followed by the visible rows; the history is the
            // (up to `rows`) lines just above them.
            let lines: Vec<&Line> = self.terminal.primary_screen().all_lines().collect();
            let above = lines.len().saturating_sub(self.rows);
            for line in &lines[above.saturating_sub(self.rows)..above] {
                out.history_row(|row| push_line(line, row));
            }
        }
        for line in self.terminal.screen().visible_lines() {
            out.row(|row| push_line(line, row));
        }
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Avt>()
}
