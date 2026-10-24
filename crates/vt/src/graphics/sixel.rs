//! The Sixel decoder (`DCS q`, DEC STD 070): a byte-at-a-time state machine that paints
//! straight to RGBA. It never fails and never panics: a byte the grammar has no rule for is
//! dropped, so a corrupt band gives wrong pixels and nothing worse.
//!
//! Supported subset: colour registers (RGB and HLS), repeat, raster attributes, graphics
//! carriage return and new line. Pixel aspect (`P1`), grid size (`P3`) and the background
//! select (`P2`) are ignored: pixels are square and untouched pixels stay transparent.

use std::fmt;

use super::MAX_DIMENSION;

/// A pixel nothing painted.
const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];
/// The buffer's row length starts here and doubles up to [`MAX_DIMENSION`].
const MIN_STRIDE: u32 = 64;
/// Arguments kept per command: the most any command reads.
const ARG_SLOTS: usize = 5;

/// VT340 registers 0 to 15 as percentages of full intensity.
const VT340_PERCENT: [[u32; 3]; 16] = [
    [0, 0, 0],
    [20, 20, 80],
    [80, 13, 13],
    [20, 80, 20],
    [80, 20, 80],
    [20, 80, 80],
    [80, 80, 20],
    [53, 53, 53],
    [26, 26, 26],
    [33, 33, 60],
    [60, 26, 26],
    [33, 60, 33],
    [60, 33, 60],
    [33, 60, 60],
    [60, 60, 33],
    [80, 80, 80],
];

/// What the grammar is reading.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Ground,
    /// `!` and its count.
    Repeat,
    /// `#` and its arguments.
    Color,
    /// `"` and its arguments.
    Raster,
}

/// The pixels of one finished image.
pub(crate) struct DecodedSixel {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `width * height * 4` bytes, row-major, straight alpha.
    pub(crate) rgba: Vec<u8>,
    /// The height of the bands above the last one: how far the cursor walks.
    pub(crate) band_pixels: u32,
}

/// One open Sixel string.
pub(crate) struct SixelDecoder {
    palette: [[u8; 4]; 256],
    color: u8,
    /// The next pixel column of the current band.
    x: u32,
    band: u32,
    /// The painted extent so far.
    width: u32,
    height: u32,
    /// The size a raster attributes command declared; it wins over the extent.
    raster: Option<(u32, u32)>,
    phase: Phase,
    args: [u32; ARG_SLOTS],
    /// Arguments seen by the open command, the unstored ones included.
    arg_count: u8,
    /// Row-major, `stride` pixels per row; `pixels.len()` is always a whole number of rows.
    pixels: Vec<[u8; 4]>,
    stride: u32,
}

impl fmt::Debug for SixelDecoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SixelDecoder")
    }
}

/// `min(value, 100)` percent of 255.
fn percent(value: u32) -> u8 {
    // At most 255.
    (value.min(100) * 255 / 100) as u8
}

/// DEC HLS to RGB: hue 0 is blue, `lightness` and `saturation` are percentages.
fn hls_to_rgb(hue: u32, lightness: u32, saturation: u32) -> [u8; 3] {
    // Reduce first: arguments saturate at `u32::MAX`, and `+ 240` on that would overflow.
    let degrees = (hue % 360 + 240) % 360;
    let lightness = lightness.min(100) as f32 / 100.0;
    let saturation = saturation.min(100) as f32 / 100.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = degrees as f32 / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match degrees / 60 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let floor = lightness - chroma / 2.0;
    let channel = |value: f32| ((value + floor) * 255.0).round().clamp(0.0, 255.0) as u8;
    [channel(r), channel(g), channel(b)]
}

impl SixelDecoder {
    pub(crate) fn new() -> SixelDecoder {
        let mut palette = [[0, 0, 0, 255]; 256];
        for (slot, [r, g, b]) in palette.iter_mut().zip(VT340_PERCENT) {
            *slot = [percent(r), percent(g), percent(b), 255];
        }
        SixelDecoder {
            palette,
            color: 0,
            x: 0,
            band: 0,
            width: 0,
            height: 0,
            raster: None,
            phase: Phase::Ground,
            args: [0; ARG_SLOTS],
            arg_count: 0,
            pixels: Vec::new(),
            stride: 0,
        }
    }

    /// Reads one byte of the string.
    pub(crate) fn put(&mut self, byte: u8) {
        match self.phase {
            Phase::Ground => self.ground(byte),
            Phase::Repeat | Phase::Color | Phase::Raster => match byte {
                b'0'..=b'9' => self.digit(byte - b'0'),
                b';' => self.arg_count = self.arg_count.saturating_add(1),
                _ => self.end_command(byte),
            },
        }
    }

    /// Ends the string. `None` when nothing is left to place.
    pub(crate) fn finish(mut self) -> Option<DecodedSixel> {
        // The string ended inside a command; it still counts.
        match self.phase {
            Phase::Color => self.define_color(),
            Phase::Raster => self.declare_raster(),
            Phase::Ground | Phase::Repeat => {}
        }
        let (width, height) = self.raster.unwrap_or((self.width, self.height));
        if width == 0 || height == 0 {
            return None;
        }
        // At most 4096 x 4096 x 4: 64 MiB.
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        let stride = self.stride as usize;
        let copied = (width as usize).min(stride);
        if copied > 0 {
            let rows = self.pixels.chunks_exact(stride);
            for (target, source) in rgba.chunks_exact_mut(width as usize * 4).zip(rows) {
                for (out, pixel) in target.chunks_exact_mut(4).zip(source.iter().take(copied)) {
                    out.copy_from_slice(pixel);
                }
            }
        }
        Some(DecodedSixel {
            width,
            height,
            rgba,
            band_pixels: self.band.saturating_mul(6).min(MAX_DIMENSION),
        })
    }

    fn ground(&mut self, byte: u8) {
        match byte {
            0x3F..=0x7E => self.paint(byte - 0x3F, 1),
            b'!' => self.begin(Phase::Repeat),
            b'#' => self.begin(Phase::Color),
            b'"' => self.begin(Phase::Raster),
            b'$' => self.x = 0,
            b'-' => {
                self.x = 0;
                self.band = self.band.saturating_add(1);
            }
            _ => {}
        }
    }

    fn begin(&mut self, phase: Phase) {
        self.phase = phase;
        self.args = [0; ARG_SLOTS];
        self.arg_count = 1;
    }

    fn digit(&mut self, digit: u8) {
        let slot = usize::from(self.arg_count).checked_sub(1);
        if let Some(arg) = slot.and_then(|slot| self.args.get_mut(slot)) {
            *arg = arg.saturating_mul(10).saturating_add(u32::from(digit));
        }
    }

    /// A byte that is not part of a command's arguments: run the command, then read the byte
    /// in the ground phase (a repeat consumes it when it is a sixel).
    fn end_command(&mut self, byte: u8) {
        let phase = std::mem::replace(&mut self.phase, Phase::Ground);
        match phase {
            Phase::Repeat => {
                if (0x3F..=0x7E).contains(&byte) {
                    let count = self.args.first().copied().unwrap_or(0).max(1);
                    self.paint(byte - 0x3F, count);
                } else {
                    self.ground(byte);
                }
            }
            Phase::Color => {
                self.define_color();
                self.ground(byte);
            }
            Phase::Raster => {
                self.declare_raster();
                self.ground(byte);
            }
            Phase::Ground => self.ground(byte),
        }
    }

    /// `#Pr` selects a register; `#Pr;Pu;Px;Py;Pz` also defines it.
    fn define_color(&mut self) {
        let [register, space, first, second, third] = self.args;
        self.color = (register & 0xFF) as u8;
        if usize::from(self.arg_count) < ARG_SLOTS {
            return;
        }
        let rgb = match space {
            2 => [percent(first), percent(second), percent(third)],
            1 => hls_to_rgb(first, second, third),
            _ => return,
        };
        if let Some(slot) = self.palette.get_mut(usize::from(self.color)) {
            *slot = [rgb[0], rgb[1], rgb[2], 255];
        }
    }

    /// `"Pan;Pad;Ph;Pv`: the declared size wins over the painted one.
    fn declare_raster(&mut self) {
        let [_, _, width, height, _] = self.args;
        if usize::from(self.arg_count) >= 4 && width > 0 && height > 0 {
            self.raster = Some((width.min(MAX_DIMENSION), height.min(MAX_DIMENSION)));
        }
    }

    /// Paints the sixel `bits` (bit 0 on top) `count` times from the current column.
    fn paint(&mut self, bits: u8, count: u32) {
        let top = self.band.saturating_mul(6);
        if top >= MAX_DIMENSION {
            return;
        }
        let x_end = self.x.saturating_add(count).min(MAX_DIMENSION);
        let y_end = (top + 6).min(MAX_DIMENSION);
        if bits != 0 {
            self.reserve(x_end, y_end);
            let color = self.palette.get(usize::from(self.color)).copied();
            for bit in 0..6u32 {
                let y = top + bit;
                if bits >> bit & 1 != 0 && y < y_end {
                    self.fill(y, x_end, color.unwrap_or(TRANSPARENT));
                }
            }
        }
        self.x = x_end;
        self.width = self.width.max(x_end);
        self.height = self.height.max(y_end);
    }

    /// Sets `x..x_end` of buffer row `y` to `color`.
    fn fill(&mut self, y: u32, x_end: u32, color: [u8; 4]) {
        let start = y as usize * self.stride as usize;
        let range = start + self.x as usize..start + x_end as usize;
        if let Some(cells) = self.pixels.get_mut(range) {
            cells.fill(color);
        }
    }

    /// Makes the buffer cover `columns` x `rows`. The row length doubles when it must grow,
    /// so the copies are amortised; it never passes [`MAX_DIMENSION`], which keeps the buffer
    /// within `MAX_DIMENSION^2` pixels (64 MiB).
    fn reserve(&mut self, columns: u32, rows: u32) {
        if columns > self.stride {
            let stride = columns
                .max(self.stride.saturating_mul(2))
                .clamp(MIN_STRIDE, MAX_DIMENSION);
            self.widen(stride);
        }
        let needed = self.stride as usize * rows as usize;
        if self.pixels.len() < needed {
            self.pixels.resize(needed, TRANSPARENT);
        }
        debug_assert!(self.pixels.len() <= (MAX_DIMENSION * MAX_DIMENSION) as usize);
    }

    /// Re-lays the buffer out with rows of `stride` pixels.
    fn widen(&mut self, stride: u32) {
        let old = self.stride as usize;
        let rows = self.pixels.len().checked_div(old).unwrap_or(0);
        let mut wider = vec![TRANSPARENT; stride as usize * rows];
        if old > 0 {
            for (target, source) in wider
                .chunks_exact_mut(stride as usize)
                .zip(self.pixels.chunks_exact(old))
            {
                if let Some(head) = target.get_mut(..old) {
                    head.copy_from_slice(source);
                }
            }
        }
        self.pixels = wider;
        self.stride = stride;
    }
}

#[cfg(test)]
#[path = "sixel_tests.rs"]
mod sixel_tests;
