//! libghostty-vt (ghostty-org/ghostty at the pinned commit) behind the comparison harness.
//!
//! The FFI declarations are written by hand (about fifteen functions); the sized structs
//! carry their own size in the first field, as the C API requires.

mod ffi;

use std::process::ExitCode;

use compare_harness::{
    Capability, Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells,
};

use crate::ffi::{CellData, GridRef, Point, StyleColor, TerminalHandle};

struct Ghostty {
    terminal: TerminalHandle,
    cols: u16,
    rows: u16,
}

fn color(color: StyleColor) -> Color {
    match color.tag {
        ffi::STYLE_COLOR_PALETTE => Color::Indexed(color.value.to_le_bytes()[0]),
        ffi::STYLE_COLOR_RGB => {
            let [r, g, b, ..] = color.value.to_le_bytes();
            Color::Rgb(r, g, b)
        }
        _ => Color::Default,
    }
}

/// The text of the cell a cell_ref points at: its codepoints, empty for a blank.
fn cell_text(cell_ref: &GridRef) -> String {
    let mut codepoints = [0u32; 32];
    let written = cell_ref.graphemes(&mut codepoints);
    codepoints[..written]
        .iter()
        .filter_map(|cp| char::from_u32(*cp))
        .filter(|c| *c != ' ' && *c != '\0')
        .collect()
}

impl Ghostty {
    fn push_row(&self, point: impl Fn(u16) -> Point, out: &mut RowCells) {
        for col in 0..self.cols {
            let Some(cell_ref) = self.terminal.grid_ref(point(col)) else {
                out.cell(Cell::default());
                continue;
            };
            let data = CellData::of(&cell_ref);
            let style = cell_ref.style();
            let uri = if data.has_hyperlink {
                cell_ref.hyperlink_uri()
            } else {
                None
            };
            let mut background = color(style.bg_color);
            // A background-only cell keeps its colour in the cell itself, not in the style.
            match data.content_tag {
                ffi::CONTENT_BG_PALETTE => background = Color::Indexed(data.palette),
                ffi::CONTENT_BG_RGB => background = Color::Rgb(data.rgb.0, data.rgb.1, data.rgb.2),
                _ => {}
            }
            out.cell(Cell {
                text: &cell_text(&cell_ref),
                wide: data.wide == ffi::WIDE_WIDE,
                spacer: data.wide == ffi::WIDE_SPACER_TAIL,
                fg: color(style.fg_color),
                bg: background,
                flags: Flags {
                    bold: style.bold,
                    italic: style.italic,
                    underline: style.underline != 0,
                    inverse: style.inverse,
                },
                uri: uri.as_deref(),
            });
        }
    }
}

impl Engine for Ghostty {
    const INFO: EngineInfo = EngineInfo {
        name: "ghostty",
        version: "1.4.1-dev",
        source: "ghostty-org/ghostty@b2c78c9",
        language: "zig",
        build: "ReleaseFast, cpu=baseline, static",
        capabilities: &[Capability::Graphemes, Capability::Hyperlinks],
        labels: &[],
    };

    fn new(geometry: Geometry) -> Self {
        let Some(terminal) = TerminalHandle::new(geometry.cols, geometry.rows, geometry.scrollback)
        else {
            eprintln!("engine-ghostty: cannot create the terminal or set its scrollback limits");
            std::process::exit(1);
        };
        Self {
            terminal,
            cols: geometry.cols,
            rows: geometry.rows,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.terminal.write(bytes);
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let (x, y) = self.terminal.cursor();
        out.cursor(usize::from(y), usize::from(x));
        if out.wants_history() {
            let history = self.terminal.scrollback_rows().min(u64::from(self.rows)) as u32;
            let first = (self.terminal.scrollback_rows() as u32).saturating_sub(history);
            for row in first..first + history {
                out.history_row(|cells| self.push_row(|col| Point::history(col, row), cells));
            }
        }
        for row in 0..u32::from(self.rows) {
            out.row(|cells| self.push_row(|col| Point::active(col, row), cells));
        }
    }

    fn scrollback_rows(&self) -> Option<u64> {
        Some(self.terminal.scrollback_rows())
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Ghostty>()
}
