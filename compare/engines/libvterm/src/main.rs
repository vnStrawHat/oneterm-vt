//! libvterm (neovim mirror, pinned commit) behind the comparison harness, with the scrollback
//! ring libvterm leaves to its embedder (label `+ring`; see `shim.c`).

use std::ffi::{c_char, c_int, c_void};
use std::process::ExitCode;

use compare_harness::{Cell, Color, DumpBuilder, Engine, EngineInfo, Flags, Geometry, RowCells};

#[repr(C)]
struct LvCell {
    chars: [u32; 6],
    width: c_int,
    bold: c_int,
    italic: c_int,
    underline: c_int,
    reverse: c_int,
    fg_kind: c_int,
    bg_kind: c_int,
    fg: [u8; 3],
    bg: [u8; 3],
}

unsafe extern "C" {
    fn lv_new(rows: c_int, cols: c_int, scrollback: usize) -> *mut c_void;
    fn lv_free(lv: *mut c_void);
    fn lv_write(lv: *mut c_void, bytes: *const c_char, len: usize) -> usize;
    fn lv_cursor(lv: *mut c_void, row: *mut c_int, col: *mut c_int);
    fn lv_cell(lv: *mut c_void, row: c_int, col: c_int, out: *mut LvCell);
    fn lv_history_len(lv: *const c_void) -> usize;
    fn lv_history_cell(lv: *const c_void, index: usize, col: c_int, out: *mut LvCell);
}

/// An owned libvterm plus its scrollback ring; freed on drop.
struct Libvterm {
    raw: *mut c_void,
    cols: c_int,
    rows: c_int,
}

impl Drop for Libvterm {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `lv_new`, is non-null and is freed once.
        unsafe { lv_free(self.raw) }
    }
}

fn color(kind: c_int, value: [u8; 3]) -> Color {
    match kind {
        1 => Color::Indexed(value[0]),
        2 => Color::Rgb(value[0], value[1], value[2]),
        _ => Color::Default,
    }
}

fn blank() -> LvCell {
    LvCell {
        chars: [0; 6],
        width: 1,
        bold: 0,
        italic: 0,
        underline: 0,
        reverse: 0,
        fg_kind: 0,
        bg_kind: 0,
        fg: [0; 3],
        bg: [0; 3],
    }
}

fn push(cell: &LvCell, out: &mut RowCells) {
    // libvterm marks the right half of a double-width character with chars[0] = -1.
    if cell.chars[0] == u32::MAX {
        out.cell(Cell {
            spacer: true,
            ..Cell::default()
        });
        return;
    }
    let text: String = cell
        .chars
        .iter()
        .take_while(|cp| **cp != 0)
        .filter_map(|cp| char::from_u32(*cp))
        .filter(|c| *c != ' ')
        .collect();
    out.cell(Cell {
        text: &text,
        wide: cell.width == 2,
        spacer: false,
        fg: color(cell.fg_kind, cell.fg),
        bg: color(cell.bg_kind, cell.bg),
        flags: Flags {
            bold: cell.bold != 0,
            italic: cell.italic != 0,
            underline: cell.underline != 0,
            inverse: cell.reverse != 0,
        },
        uri: None,
    });
}

impl Engine for Libvterm {
    const INFO: EngineInfo = EngineInfo {
        name: "libvterm",
        version: "0.3 (neovim mirror)",
        source: "neovim/libvterm@934bc2f",
        language: "c",
        build: "MSVC /O2, static, scrollback ring in the adapter",
        capabilities: &[],
        labels: &["+ring"],
    };

    fn new(geometry: Geometry) -> Self {
        // SAFETY: plain value arguments; a null result is handled below.
        let raw = unsafe {
            lv_new(
                c_int::from(geometry.rows),
                c_int::from(geometry.cols),
                geometry.scrollback,
            )
        };
        if raw.is_null() {
            eprintln!("engine-libvterm: cannot create the terminal and its scrollback ring");
            std::process::exit(1);
        }
        Self {
            raw,
            cols: c_int::from(geometry.cols),
            rows: c_int::from(geometry.rows),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        // SAFETY: the pointer/length pair is a live slice; `raw` is valid. libvterm copies what
        // it needs before returning.
        let consumed = unsafe { lv_write(self.raw, bytes.as_ptr().cast(), bytes.len()) };
        debug_assert_eq!(consumed, bytes.len());
    }

    fn dump(&self, out: &mut DumpBuilder) {
        let (mut row, mut col) = (0, 0);
        // SAFETY: both out-pointers are live locals; `raw` is valid.
        unsafe { lv_cursor(self.raw, &raw mut row, &raw mut col) };
        out.cursor(
            usize::try_from(row).unwrap_or(0),
            usize::try_from(col).unwrap_or(0),
        );
        if out.wants_history() {
            // SAFETY: `raw` is valid.
            let held = unsafe { lv_history_len(self.raw) };
            let count = held.min(usize::try_from(self.rows).unwrap_or(0));
            for index in held - count..held {
                out.history_row(|cells| {
                    for column in 0..self.cols {
                        let mut cell = blank();
                        // SAFETY: `index < held` and `column < cols`, so the slot exists; `cell`
                        // is a live local of the C struct's layout.
                        unsafe { lv_history_cell(self.raw, index, column, &raw mut cell) };
                        push(&cell, cells);
                    }
                });
            }
        }
        for r in 0..self.rows {
            out.row(|cells| {
                for column in 0..self.cols {
                    let mut cell = blank();
                    // SAFETY: the position is on the screen; `cell` is a live local.
                    unsafe { lv_cell(self.raw, r, column, &raw mut cell) };
                    push(&cell, cells);
                }
            });
        }
    }
}

fn main() -> ExitCode {
    compare_harness::main::<Libvterm>()
}
