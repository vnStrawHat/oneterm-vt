//! wezterm-term (wezterm/wezterm at the pinned commit) behind the comparison harness.

use std::process::ExitCode;
use std::sync::Arc;

use compare_harness::{
    Capability, Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells,
};
use wezterm_term::color::{ColorAttribute, ColorPalette};
use wezterm_term::{
    CellAttributes, Intensity, Line, Terminal, TerminalConfiguration, TerminalSize, Underline,
};

#[derive(Debug)]
struct Configuration {
    scrollback: usize,
}

impl TerminalConfiguration for Configuration {
    fn scrollback_size(&self) -> usize {
        self.scrollback
    }

    fn color_palette(&self) -> ColorPalette {
        ColorPalette::default()
    }
}

struct Wezterm {
    terminal: Terminal,
    cols: usize,
    rows: usize,
}

fn color(color: ColorAttribute) -> Color {
    match color {
        ColorAttribute::Default => Color::Default,
        ColorAttribute::PaletteIndex(index) => Color::Indexed(index),
        ColorAttribute::TrueColorWithPaletteFallback(rgba, _)
        | ColorAttribute::TrueColorWithDefaultFallback(rgba) => {
            let (r, g, b, _) = rgba.to_srgb_u8();
            Color::Rgb(r, g, b)
        }
    }
}

fn flags(attrs: &CellAttributes) -> Flags {
    Flags {
        bold: attrs.intensity() == Intensity::Bold,
        italic: attrs.italic(),
        underline: attrs.underline() != Underline::None,
        inverse: attrs.reverse(),
    }
}

/// One row: `visible_cells` leaves out the blank that follows a wide cell, which becomes the
/// spacer column; a short line is padded with default blanks.
fn push_line(line: &Line, cols: usize, out: &mut RowCells) {
    let mut column = 0;
    for cell in line.visible_cells() {
        while column < cell.cell_index() && column < cols {
            out.cell(Cell {
                spacer: true,
                ..Cell::default()
            });
            column += 1;
        }
        if column >= cols {
            break;
        }
        let attrs = cell.attrs();
        let text = cell.str();
        out.cell(Cell {
            text: if text == " " { "" } else { text },
            wide: cell.width() > 1,
            spacer: false,
            fg: color(attrs.foreground()),
            bg: color(attrs.background()),
            flags: flags(attrs),
            uri: attrs.hyperlink().map(|link| link.uri()),
        });
        column += 1;
    }
    while column < cols {
        out.cell(Cell::default());
        column += 1;
    }
}

impl Engine for Wezterm {
    const INFO: EngineInfo = EngineInfo {
        name: "wezterm",
        version: "0.1.0",
        source: "wezterm/wezterm@e68a8ec, package wezterm-term",
        language: "rust",
        build: "release, thin LTO, path dependency",
        capabilities: &[
            Capability::Sixel,
            Capability::Graphemes,
            Capability::Hyperlinks,
        ],
        labels: &[],
    };

    fn new(geometry: Geometry) -> Self {
        let (cols, rows) = (usize::from(geometry.cols), usize::from(geometry.rows));
        let size = TerminalSize {
            rows,
            cols,
            pixel_width: cols * 10,
            pixel_height: rows * 20,
            dpi: 96,
        };
        let config = Arc::new(Configuration {
            scrollback: geometry.scrollback,
        });
        Self {
            terminal: Terminal::new(size, config, "compare", "0", Box::new(std::io::sink())),
            cols,
            rows,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.terminal.advance_bytes(bytes);
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let cursor = self.terminal.cursor_pos();
        out.cursor(usize::try_from(cursor.y).unwrap_or(0), cursor.x);
        let screen = self.terminal.screen();
        let total = screen.scrollback_rows();
        let top = total.saturating_sub(self.rows);
        if out.wants_history() {
            let from = top.saturating_sub(self.rows);
            for line in screen.lines_in_phys_range(from..top) {
                out.history_row(|row| push_line(&line, self.cols, row));
            }
        }
        for line in screen.lines_in_phys_range(top..total) {
            out.row(|row| push_line(&line, self.cols, row));
        }
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Wezterm>()
}
