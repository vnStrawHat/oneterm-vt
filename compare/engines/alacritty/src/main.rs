//! alacritty_terminal 0.26.0 (vte 0.15.0) behind the comparison harness.

use std::process::ExitCode;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags as CellFlags};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Color as TermColor, Processor, StdSyncHandler};
use compare_harness::{
    Capability, Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells,
};

struct Alacritty {
    term: Term<VoidListener>,
    processor: Processor<StdSyncHandler>,
}

fn color(color: TermColor) -> Color {
    match color {
        TermColor::Indexed(index) => Color::Indexed(index),
        TermColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        // Black..BrightWhite are palette 0-15; the foreground, background, cursor and dim
        // slots are the default colour.
        TermColor::Named(named) => match named as usize {
            slot @ 0..16 => Color::Indexed(slot as u8),
            _ => Color::Default,
        },
    }
}

fn push_cell(row: &mut RowCells, cell: &TermCell) {
    if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
        row.cell(Cell {
            spacer: true,
            ..Cell::default()
        });
        return;
    }
    let mut text = String::new();
    if cell.c != ' ' {
        text.push(cell.c);
    }
    if let Some(marks) = cell.zerowidth() {
        // A blank base is a space once a mark is stored beside it.
        if text.is_empty() && !marks.is_empty() {
            text.push(' ');
        }
        text.extend(marks);
    }
    let link = cell.hyperlink();
    row.cell(Cell {
        text: &text,
        wide: cell.flags.contains(CellFlags::WIDE_CHAR),
        spacer: false,
        fg: color(cell.fg),
        bg: color(cell.bg),
        flags: Flags {
            bold: cell.flags.contains(CellFlags::BOLD),
            italic: cell.flags.contains(CellFlags::ITALIC),
            underline: cell.flags.intersects(CellFlags::ALL_UNDERLINES),
            inverse: cell.flags.contains(CellFlags::INVERSE),
        },
        uri: link.as_ref().map(|link| link.uri()),
    });
}

fn push_line(grid: &Grid<TermCell>, line: Line, out: &mut RowCells) {
    for column in 0..grid.columns() {
        push_cell(out, &grid[line][Column(column)]);
    }
}

impl Engine for Alacritty {
    const INFO: EngineInfo = EngineInfo {
        name: "alacritty",
        version: "0.26.0",
        source: "crates.io alacritty_terminal =0.26.0 (vte 0.15.0)",
        language: "rust",
        build: "release, thin LTO",
        capabilities: &[Capability::Hyperlinks],
        labels: &[],
    };

    fn new(geometry: Geometry) -> Self {
        let config = Config {
            scrolling_history: geometry.scrollback,
            ..Config::default()
        };
        let size = TermSize::new(usize::from(geometry.cols), usize::from(geometry.rows));
        Self {
            term: Term::new(config, &size, VoidListener),
            processor: Processor::new(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    fn finish(&mut self) {
        // vte holds the bytes of a `?2026` window until it closes or is flushed.
        if self.processor.sync_bytes_count() > 0 {
            self.processor.stop_sync(&mut self.term);
        }
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        out.cursor(usize::try_from(cursor.line.0).unwrap_or(0), cursor.column.0);
        if out.wants_history() {
            let history = grid.history_size().min(grid.screen_lines());
            for k in (1..=history).rev() {
                let line = Line(-i32::try_from(k).unwrap_or(0));
                out.history_row(|row| push_line(grid, line, row));
            }
        }
        for line in 0..grid.screen_lines() {
            let line = Line(i32::try_from(line).unwrap_or(0));
            out.row(|row| push_line(grid, line, row));
        }
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Alacritty>()
}
