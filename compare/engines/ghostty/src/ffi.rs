//! The hand-written libghostty-vt bindings this adapter uses, and safe wrappers over them.
//!
//! Every wrapper keeps the invariants its `unsafe` block needs: the terminal pointer is
//! non-null and owned by `TerminalHandle`; out-parameters are local, initialised, and the size
//! passed with a buffer is the size of that buffer; a grid ref is read right after it
//! is created and before the terminal is written again.

use std::ffi::c_void;

const SUCCESS: i32 = 0;

// GhosttyTerminalOption
const OPT_SCROLLBACK_MAX_BYTES: i32 = 27;
const OPT_SCROLLBACK_MAX_LINES: i32 = 28;
// GhosttyTerminalData
const DATA_CURSOR_X: i32 = 3;
const DATA_CURSOR_Y: i32 = 4;
const DATA_SCROLLBACK_ROWS: i32 = 15;
// GhosttyPointTag
const POINT_ACTIVE: i32 = 0;
const POINT_HISTORY: i32 = 3;
// GhosttyCellData
const CELL_CONTENT_TAG: i32 = 2;
const CELL_WIDE: i32 = 3;
const CELL_HAS_HYPERLINK: i32 = 7;
const CELL_COLOR_PALETTE: i32 = 10;
const CELL_COLOR_RGB: i32 = 11;

pub const CONTENT_BG_PALETTE: i32 = 2;
pub const CONTENT_BG_RGB: i32 = 3;
pub const WIDE_WIDE: i32 = 1;
pub const WIDE_SPACER_TAIL: i32 = 2;
pub const STYLE_COLOR_PALETTE: i32 = 1;
pub const STYLE_COLOR_RGB: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StyleColor {
    pub tag: i32,
    /// The C union: a palette index in byte 0, or r, g, b in bytes 0-2.
    pub value: u64,
}

// The unread fields exist to give the C struct its layout.
#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Style {
    size: usize,
    pub fg_color: StyleColor,
    pub bg_color: StyleColor,
    underline_color: StyleColor,
    pub bold: bool,
    pub italic: bool,
    faint: bool,
    blink: bool,
    pub inverse: bool,
    invisible: bool,
    strikethrough: bool,
    overline: bool,
    pub underline: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Point {
    tag: i32,
    /// The C union, 16 bytes: x (u16) at byte 0, y (u32) at byte 4.
    value: [u64; 2],
}

impl Point {
    fn new(tag: i32, x: u16, y: u32) -> Self {
        Point {
            tag,
            value: [u64::from(x) | u64::from(y) << 32, 0],
        }
    }

    pub fn active(x: u16, y: u32) -> Self {
        Self::new(POINT_ACTIVE, x, y)
    }

    pub fn history(x: u16, y: u32) -> Self {
        Self::new(POINT_HISTORY, x, y)
    }
}

#[repr(C)]
pub struct GridRef {
    size: usize,
    node: *mut c_void,
    x: u16,
    y: u16,
}

unsafe extern "C" {
    fn ghostty_terminal_new(
        allocator: *const c_void,
        terminal: *mut *mut c_void,
        cols: u16,
        rows: u16,
    ) -> i32;
    fn ghostty_terminal_free(terminal: *mut c_void);
    fn ghostty_terminal_set(terminal: *mut c_void, option: i32, value: *const c_void) -> i32;
    fn ghostty_terminal_get(terminal: *mut c_void, data: i32, out: *mut c_void) -> i32;
    fn ghostty_terminal_vt_write(terminal: *mut c_void, data: *const u8, len: usize);
    fn ghostty_terminal_grid_ref(terminal: *mut c_void, point: Point, out: *mut GridRef) -> i32;
    fn ghostty_grid_ref_cell(cell_ref: *const GridRef, out: *mut u64) -> i32;
    fn ghostty_grid_ref_style(cell_ref: *const GridRef, out: *mut Style) -> i32;
    fn ghostty_grid_ref_graphemes(
        cell_ref: *const GridRef,
        buf: *mut u32,
        buf_len: usize,
        out_len: *mut usize,
    ) -> i32;
    fn ghostty_grid_ref_hyperlink_uri(
        cell_ref: *const GridRef,
        buf: *mut u8,
        buf_len: usize,
        out_len: *mut usize,
    ) -> i32;
    fn ghostty_cell_get(cell: u64, data: i32, out: *mut c_void) -> i32;
}

/// An owned terminal; freed on drop.
pub struct TerminalHandle(*mut c_void);

impl Drop for TerminalHandle {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `ghostty_terminal_new`, is non-null and is freed once.
        unsafe { ghostty_terminal_free(self.0) }
    }
}

impl TerminalHandle {
    /// Creates the terminal and, before the first write, removes the default 10 000-BYTE
    /// scrollback cap and sets a line limit instead (the cap keeps almost no history).
    /// `None` if any call fails.
    pub fn new(cols: u16, rows: u16, scrollback_lines: usize) -> Option<Self> {
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: `raw` is a local out-pointer; a null allocator selects the default one.
        let created = unsafe { ghostty_terminal_new(std::ptr::null(), &raw mut raw, cols, rows) };
        if created != SUCCESS || raw.is_null() {
            return None;
        }
        let handle = TerminalHandle(raw);
        // SAFETY: the terminal is valid; a null value removes the byte limit, and a pointer to
        // a local `usize` is the documented input for the line limit.
        let (bytes, lines) = unsafe {
            (
                ghostty_terminal_set(raw, OPT_SCROLLBACK_MAX_BYTES, std::ptr::null()),
                ghostty_terminal_set(
                    raw,
                    OPT_SCROLLBACK_MAX_LINES,
                    (&raw const scrollback_lines).cast(),
                ),
            )
        };
        (bytes == SUCCESS && lines == SUCCESS).then_some(handle)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        // SAFETY: the pointer/length pair is a live slice; the terminal is valid.
        unsafe { ghostty_terminal_vt_write(self.0, bytes.as_ptr(), bytes.len()) }
    }

    fn get<T: Default>(&self, data: i32) -> T {
        let mut value = T::default();
        // SAFETY: every `data` id used here documents an output type equal to `T`, and
        // `value` is a local of that type.
        unsafe { ghostty_terminal_get(self.0, data, (&raw mut value).cast()) };
        value
    }

    /// Cursor `(x, y)` on the active screen, 0-based.
    pub fn cursor(&self) -> (u16, u16) {
        (self.get(DATA_CURSOR_X), self.get(DATA_CURSOR_Y))
    }

    /// History rows of the active screen.
    pub fn scrollback_rows(&self) -> u64 {
        self.get::<usize>(DATA_SCROLLBACK_ROWS) as u64
    }

    /// A snapshot of one cell; valid until the terminal is written again.
    pub fn grid_ref(&self, point: Point) -> Option<GridRef> {
        let mut cell_ref = GridRef {
            size: size_of::<GridRef>(),
            node: std::ptr::null_mut(),
            x: 0,
            y: 0,
        };
        // SAFETY: `cell_ref` is a local, sized struct with its `size` field set.
        let ok = unsafe { ghostty_terminal_grid_ref(self.0, point, &raw mut cell_ref) };
        (ok == SUCCESS).then_some(cell_ref)
    }
}

impl GridRef {
    pub fn style(&self) -> Style {
        let mut style = Style {
            size: size_of::<Style>(),
            fg_color: StyleColor { tag: 0, value: 0 },
            bg_color: StyleColor { tag: 0, value: 0 },
            underline_color: StyleColor { tag: 0, value: 0 },
            bold: false,
            italic: false,
            faint: false,
            blink: false,
            inverse: false,
            invisible: false,
            strikethrough: false,
            overline: false,
            underline: 0,
        };
        // SAFETY: `style` is a local, sized struct with its `size` field set.
        unsafe { ghostty_grid_ref_style(self, &raw mut style) };
        style
    }

    /// The codepoints of the cell into `buf`; how many were written (0 for a blank).
    pub fn graphemes(&self, buf: &mut [u32]) -> usize {
        let mut written = 0usize;
        // SAFETY: `buf` is a live buffer of `buf.len()` elements; `written` is a local.
        let result = unsafe {
            ghostty_grid_ref_graphemes(self, buf.as_mut_ptr(), buf.len(), &raw mut written)
        };
        if result == SUCCESS {
            written.min(buf.len())
        } else {
            0
        }
    }

    /// The hyperlink target, if the cell has one.
    pub fn hyperlink_uri(&self) -> Option<String> {
        let mut buf = vec![0u8; 2048];
        let mut written = 0usize;
        // SAFETY: `buf` is a live buffer of `buf.len()` bytes; `written` is a local.
        let mut result = unsafe {
            ghostty_grid_ref_hyperlink_uri(self, buf.as_mut_ptr(), buf.len(), &raw mut written)
        };
        if result != SUCCESS && written > buf.len() {
            // Out of space: `written` is the required size.
            buf.resize(written, 0);
            // SAFETY: as above, with the buffer grown to the size the library asked for.
            result = unsafe {
                ghostty_grid_ref_hyperlink_uri(self, buf.as_mut_ptr(), buf.len(), &raw mut written)
            };
        }
        (result == SUCCESS && written > 0)
            .then(|| String::from_utf8_lossy(&buf[..written.min(buf.len())]).into_owned())
    }
}

/// The cell fields the adapter reads.
pub struct CellData {
    pub content_tag: i32,
    pub wide: i32,
    pub has_hyperlink: bool,
    pub palette: u8,
    pub rgb: (u8, u8, u8),
}

impl CellData {
    pub fn of(cell_ref: &GridRef) -> Self {
        let mut cell = 0u64;
        // SAFETY: `cell` is a local `GhosttyCell` (uint64_t).
        let ok = unsafe { ghostty_grid_ref_cell(cell_ref, &raw mut cell) };
        let mut data = CellData {
            content_tag: 0,
            wide: 0,
            has_hyperlink: false,
            palette: 0,
            rgb: (0, 0, 0),
        };
        if ok != SUCCESS {
            return data;
        }
        let mut rgb = [0u8; 3];
        // SAFETY: each id documents its output type (`int`-sized enum, bool, uint8, three
        // bytes); every out-pointer is a local of that size.
        unsafe {
            ghostty_cell_get(cell, CELL_CONTENT_TAG, (&raw mut data.content_tag).cast());
            ghostty_cell_get(cell, CELL_WIDE, (&raw mut data.wide).cast());
            ghostty_cell_get(
                cell,
                CELL_HAS_HYPERLINK,
                (&raw mut data.has_hyperlink).cast(),
            );
            if data.content_tag == CONTENT_BG_PALETTE {
                ghostty_cell_get(cell, CELL_COLOR_PALETTE, (&raw mut data.palette).cast());
            }
            if data.content_tag == CONTENT_BG_RGB {
                ghostty_cell_get(cell, CELL_COLOR_RGB, rgb.as_mut_ptr().cast());
            }
        }
        data.rgb = (rgb[0], rgb[1], rgb[2]);
        data
    }
}
