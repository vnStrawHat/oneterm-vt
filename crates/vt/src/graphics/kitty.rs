//! The kitty graphics protocol on the wire: `APC G` control keys, chunked base64 payloads and
//! the three pixel formats, decoded to RGBA.
//!
//! Nothing here knows the grid. The reader turns the bytes of `APC` strings into a [`Load`]
//! (one command and its payload, however many chunks carried it); [`Load::decode`] gives the
//! pixels; the dispatcher stores, places and deletes.
//!
//! An image and a placement are two things, as in the protocol: the pixels are decoded once
//! and shared, and a placement says which part of them goes where.
//!
//! An image and a placement are two things, as in the protocol: the pixels are decoded once
//! and shared, and a placement says which part of them goes where.

use std::io::Cursor;

use miniz_oxide::inflate::{TINFLStatus, decompress_to_vec_zlib_with_limit};

use super::MAX_DIMENSION;
use super::image_store::{ImageStore, StoredImage};
use crate::grid::ScreenKind;
use crate::intern::GraphicId;

/// Most decoded payload bytes of one load: a 4096 x 4096 RGBA image.
pub(crate) const LOAD_MAX_BYTES: usize = (MAX_DIMENSION as usize) * (MAX_DIMENSION as usize) * 4;
/// Most control-data bytes of one command.
const CONTROL_MAX_BYTES: usize = 512;

/// Why a command was refused: the reply message, `CODE:reason`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Refusal(pub(crate) &'static str);

const BAD_PNG: Refusal = Refusal("EBADPNG:the payload is not a PNG image this terminal reads");
const TOO_LARGE: Refusal = Refusal("EFBIG:the payload is larger than 64 MiB");
const BAD_SIZE: Refusal = Refusal("EINVAL:an image side must be between 1 and 4096 pixels");

/// The control data of one command. A key the protocol defines but this engine does not act
/// on is not kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Command {
    /// `a`.
    pub(crate) action: u8,
    /// `q`: 1 silences `OK`, 2 silences everything.
    pub(crate) quiet: u32,
    /// `f`: 24, 32 or 100.
    pub(crate) format: u32,
    /// `t`.
    pub(crate) medium: u8,
    /// `o`, 0 when absent.
    pub(crate) compression: u8,
    /// `m=1`.
    pub(crate) more: bool,
    /// `s` and `v`: the pixel size of raw data.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `i`, `I`, `p`.
    pub(crate) image_id: u32,
    pub(crate) image_number: u32,
    pub(crate) placement_id: u32,
    /// `x`, `y`: the source rectangle's corner, or the cell a delete names.
    pub(crate) x: u32,
    pub(crate) y: u32,
    /// `w`, `h`: the source rectangle's size, 0 for "to the edge".
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
    /// `X`, `Y`: the pixel offset inside the first cell.
    pub(crate) cell_x: u32,
    pub(crate) cell_y: u32,
    /// `c`, `r`: the cells to draw over, 0 for "as the pixels need".
    pub(crate) cols: u32,
    pub(crate) rows: u32,
    /// `z`.
    pub(crate) z_index: i32,
    /// `C=1`.
    pub(crate) cursor_stays: bool,
    /// `d`.
    pub(crate) delete: u8,
    /// `U` set: a Unicode placeholder placement.
    pub(crate) placeholder: bool,
    /// `P` or `Q` set: a placement relative to another one.
    pub(crate) relative: bool,
}

impl Default for Command {
    fn default() -> Self {
        Command {
            action: b't',
            quiet: 0,
            format: 32,
            medium: b'd',
            compression: 0,
            more: false,
            width: 0,
            height: 0,
            image_id: 0,
            image_number: 0,
            placement_id: 0,
            x: 0,
            y: 0,
            source_width: 0,
            source_height: 0,
            cell_x: 0,
            cell_y: 0,
            cols: 0,
            rows: 0,
            z_index: 0,
            cursor_stays: false,
            delete: b'a',
            placeholder: false,
            relative: false,
        }
    }
}

impl Command {
    /// Parses control data: `key=value` pairs separated by commas. `None` for a malformed
    /// pair; a key this engine does not know is skipped.
    pub(crate) fn parse(control: &[u8]) -> Option<Command> {
        let mut command = Command::default();
        for pair in control.split(|byte| *byte == b',') {
            let (key, value) = match pair {
                [] => continue,
                [key, b'=', value @ ..] if !value.is_empty() => (*key, value),
                _ => return None,
            };
            match key {
                b'a' => command.action = letter(value)?,
                b't' => command.medium = letter(value)?,
                b'o' => command.compression = letter(value)?,
                b'd' => command.delete = letter(value)?,
                b'z' => command.z_index = signed(value)?,
                b'q' => command.quiet = unsigned(value)?,
                b'f' => command.format = unsigned(value)?,
                b'm' => command.more = unsigned(value)? == 1,
                b's' => command.width = unsigned(value)?,
                b'v' => command.height = unsigned(value)?,
                b'i' => command.image_id = unsigned(value)?,
                b'I' => command.image_number = unsigned(value)?,
                b'p' => command.placement_id = unsigned(value)?,
                b'x' => command.x = unsigned(value)?,
                b'y' => command.y = unsigned(value)?,
                b'w' => command.source_width = unsigned(value)?,
                b'h' => command.source_height = unsigned(value)?,
                b'X' => command.cell_x = unsigned(value)?,
                b'Y' => command.cell_y = unsigned(value)?,
                b'c' => command.cols = unsigned(value)?,
                b'r' => command.rows = unsigned(value)?,
                b'C' => command.cursor_stays = unsigned(value)? == 1,
                b'U' => command.placeholder = unsigned(value)? != 0,
                b'P' | b'Q' => command.relative |= unsigned(value)? != 0,
                _ => {}
            }
        }
        Some(command)
    }
}

fn letter(value: &[u8]) -> Option<u8> {
    match value {
        [byte] => Some(*byte),
        _ => None,
    }
}

fn unsigned(value: &[u8]) -> Option<u32> {
    let mut number = 0u32;
    for byte in value {
        let digit = u32::from(byte.checked_sub(b'0').filter(|digit| *digit < 10)?);
        number = number.checked_mul(10)?.checked_add(digit)?;
    }
    Some(number)
}

fn signed(value: &[u8]) -> Option<i32> {
    match value {
        [b'-', digits @ ..] if !digits.is_empty() => {
            // `-2147483648` has no positive counterpart in `i32`, so negate in `i64`.
            i32::try_from(-i64::from(unsigned(digits)?)).ok()
        }
        _ => i32::try_from(unsigned(value)?).ok(),
    }
}

/// The value of a base64 character, or `NOT_BASE64`.
const BASE64: [u8; 256] = {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [NOT_BASE64; 256];
    let mut value = 0;
    while value < alphabet.len() {
        table[alphabet[value] as usize] = value as u8;
        value += 1;
    }
    table
};
const NOT_BASE64: u8 = 0xFF;

/// One command with the payload its chunks carried.
#[derive(Debug)]
pub(crate) struct Load {
    /// The control data of the first chunk.
    pub(crate) command: Command,
    /// The decoded payload so far.
    data: Vec<u8>,
    /// Whether the newest chunk announced another one.
    more: bool,
    /// The payload passed `LOAD_MAX_BYTES`; the rest was dropped.
    too_large: bool,
    /// A payload byte was outside the base64 alphabet.
    bad_base64: bool,
}

/// RGBA8 pixels, row-major, straight alpha: `rgba.len() == width * height * 4`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Image {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

impl Load {
    /// Decodes the payload as the command's format says.
    pub(crate) fn decode(self) -> Result<Image, Refusal> {
        if self.bad_base64 {
            return Err(Refusal("EINVAL:the payload is not base64"));
        }
        if self.too_large {
            return Err(TOO_LARGE);
        }
        let zlib = match self.command.compression {
            0 => false,
            b'z' => true,
            _ => return Err(Refusal("EINVAL:unknown compression")),
        };
        match self.command.format {
            24 => raw_image(&self.command, self.data, zlib, 3),
            32 => raw_image(&self.command, self.data, zlib, 4),
            100 if zlib => decode_png(&inflate(&self.data, LOAD_MAX_BYTES).ok_or(TOO_LARGE)?),
            100 => decode_png(&self.data),
            _ => Err(Refusal("EINVAL:unknown format")),
        }
    }
}

/// Inflates a zlib stream into at most `limit` bytes. `None` when the stream is broken; a
/// stream that holds more gives its first `limit` bytes, as raw data past the image is ignored.
fn inflate(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    match decompress_to_vec_zlib_with_limit(data, limit) {
        Ok(output) => Some(output),
        Err(error) if error.status == TINFLStatus::HasMoreOutput => Some(error.output),
        Err(_) => None,
    }
}

/// `f=24` and `f=32`: `channels` bytes per pixel, `s` by `v` pixels.
fn raw_image(
    command: &Command,
    data: Vec<u8>,
    zlib: bool,
    channels: usize,
) -> Result<Image, Refusal> {
    const SHORT: Refusal = Refusal("ENODATA:the payload is shorter than the image");
    let (width, height) = (command.width, command.height);
    if !side_fits(width) || !side_fits(height) {
        return Err(BAD_SIZE);
    }
    // At most 4096 * 4096 * 4.
    let pixels = width as usize * height as usize;
    let needed = pixels * channels;
    let mut data = if zlib {
        inflate(&data, needed).ok_or(SHORT)?
    } else {
        data
    };
    if data.len() < needed {
        return Err(SHORT);
    }
    data.truncate(needed);
    let rgba = if channels == 4 {
        data
    } else {
        widen(&data, 3, |rgb| [rgb[0], rgb[1], rgb[2], 255])
    };
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn side_fits(side: u32) -> bool {
    (1..=MAX_DIMENSION).contains(&side)
}

/// Maps every `channels`-byte pixel of `data` to RGBA.
fn widen(data: &[u8], channels: usize, to_rgba: impl Fn(&[u8]) -> [u8; 4]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(data.len() / channels * 4);
    for pixel in data.chunks_exact(channels) {
        rgba.extend_from_slice(&to_rgba(pixel));
    }
    rgba
}

/// `f=100`: the first frame of a PNG, as RGBA8.
fn decode_png(data: &[u8]) -> Result<Image, Refusal> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    // Palette and low-depth grey expanded, `tRNS` to alpha, 16-bit samples cut to 8.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|_| BAD_PNG)?;
    let (width, height) = (reader.info().width, reader.info().height);
    // Checked before the pixels are allocated: the header alone can declare 2^31 pixels a side.
    if !side_fits(width) || !side_fits(height) {
        return Err(BAD_SIZE);
    }
    let mut pixels = vec![0; reader.output_buffer_size().ok_or(BAD_PNG)?];
    let frame = reader.next_frame(&mut pixels).map_err(|_| BAD_PNG)?;
    pixels.truncate(frame.buffer_size());
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(BAD_PNG);
    }
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => widen(&pixels, 3, |rgb| [rgb[0], rgb[1], rgb[2], 255]),
        png::ColorType::Grayscale => widen(&pixels, 1, |grey| [grey[0], grey[0], grey[0], 255]),
        png::ColorType::GrayscaleAlpha => {
            widen(&pixels, 2, |grey| [grey[0], grey[0], grey[0], grey[1]])
        }
        png::ColorType::Indexed => return Err(BAD_PNG),
    };
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(BAD_PNG);
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// The part of an image a placement shows, in image pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SourceRect {
    pub(crate) origin: (u32, u32),
    pub(crate) size: (u32, u32),
}

/// The source rectangle of `command` inside an image of `size` pixels: `x`, `y` must lie
/// inside the image, `w` and `h` reach to the edge when they are 0 or too long.
pub(crate) fn source_rect(command: &Command, size: (u32, u32)) -> Result<SourceRect, Refusal> {
    let (x, y) = (command.x, command.y);
    if x >= size.0 || y >= size.1 {
        return Err(Refusal("EINVAL:the source rectangle is outside the image"));
    }
    let fit = |wanted: u32, room: u32| if wanted == 0 { room } else { wanted.min(room) };
    let width = fit(command.source_width, size.0 - x);
    let height = fit(command.source_height, size.1 - y);
    Ok(SourceRect {
        origin: (x, y),
        size: (width, height),
    })
}

/// Where the reader is inside an `APC` string.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Phase {
    /// Between strings, or inside one that is not a graphics command.
    #[default]
    Ignoring,
    /// The byte after the introducer: `G` makes it a graphics command.
    Introducer,
    /// Control data, up to the first `;`.
    Control,
    /// Base64 payload.
    Payload,
}

/// What the end of an `APC` string means to the dispatcher.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// Not a graphics command, a malformed one, or an aborted string.
    Ignored,
    /// A chunk that announced another one.
    Chunk,
    /// A whole command.
    Command(Load),
}

/// A kitty placement: which stored image it shows, under which placement id.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PlacementRef {
    pub(crate) graphic: GraphicId,
    pub(crate) screen: ScreenKind,
    /// The image id, 0 for an image that was never stored.
    pub(crate) image: u32,
    /// `p`, 0 when the program gave none.
    pub(crate) placement: u32,
    pub(crate) z_index: i32,
}

/// The kitty side of a terminal's graphics state.
#[derive(Debug, Default)]
pub(crate) struct KittyState {
    phase: Phase,
    control: Vec<u8>,
    /// Bits of the base64 group being read, and how many.
    bits: u32,
    bit_count: u32,
    /// The load the chunks so far belong to.
    load: Option<Load>,
    pub(crate) store: ImageStore,
    /// One entry per live kitty placement; `GraphicsState::release` removes it.
    pub(crate) refs: Vec<PlacementRef>,
}

impl KittyState {
    /// The start of an `APC`, `SOS` or `PM` string; only `APC` (`_`) can hold a command.
    pub(crate) fn start(&mut self, introducer: u8) {
        self.phase = if introducer == b'_' {
            Phase::Introducer
        } else {
            Phase::Ignoring
        };
        self.control.clear();
    }

    /// One payload byte of the string.
    pub(crate) fn put(&mut self, byte: u8) {
        match self.phase {
            Phase::Ignoring => {}
            Phase::Introducer => {
                self.phase = if byte == b'G' {
                    Phase::Control
                } else {
                    Phase::Ignoring
                };
            }
            Phase::Control if byte == b';' => self.open_payload(),
            Phase::Control if self.control.len() < CONTROL_MAX_BYTES => self.control.push(byte),
            Phase::Control => {
                // Control data this long is no command; a load it would have continued is lost.
                self.load = None;
                self.phase = Phase::Ignoring;
            }
            Phase::Payload => self.put_base64(byte),
        }
    }

    /// The control data is complete: this string opens a load or continues the open one.
    fn open_payload(&mut self) {
        let command = Command::parse(&self.control);
        match (&mut self.load, command) {
            // A continuation: only `m` counts, and a chunk without usable keys is the last.
            (Some(load), command) => load.more = command.is_some_and(|command| command.more),
            (None, Some(command)) => {
                self.load = Some(Load {
                    command,
                    data: Vec::new(),
                    more: command.more,
                    too_large: false,
                    bad_base64: false,
                });
            }
            (None, None) => {
                self.phase = Phase::Ignoring;
                return;
            }
        }
        self.phase = Phase::Payload;
        (self.bits, self.bit_count) = (0, 0);
    }

    fn put_base64(&mut self, byte: u8) {
        let Some(load) = &mut self.load else {
            return;
        };
        if byte == b'=' {
            (self.bits, self.bit_count) = (0, 0);
            return;
        }
        let value = BASE64[usize::from(byte)];
        if value == NOT_BASE64 {
            load.bad_base64 = true;
            return;
        }
        self.bits = (self.bits << 6) | u32::from(value);
        self.bit_count += 6;
        if self.bit_count < 8 {
            return;
        }
        self.bit_count -= 8;
        if load.data.len() >= LOAD_MAX_BYTES {
            load.too_large = true;
            return;
        }
        // Truncation keeps the eight bits just completed.
        load.data.push((self.bits >> self.bit_count) as u8);
    }

    /// The end of the string.
    pub(crate) fn end(&mut self, aborted: bool) -> Outcome {
        if self.phase == Phase::Control && !aborted {
            // A command without payload has no `;`.
            self.open_payload();
        }
        let phase = std::mem::take(&mut self.phase);
        if aborted {
            if matches!(phase, Phase::Control | Phase::Payload) {
                self.load = None;
            }
            return Outcome::Ignored;
        }
        if phase != Phase::Payload {
            return Outcome::Ignored;
        }
        match self.load.take() {
            Some(load) if load.more => {
                self.load = Some(load);
                Outcome::Chunk
            }
            Some(load) => Outcome::Command(load),
            None => Outcome::Ignored,
        }
    }

    /// RIS: forgets any load in flight and the stored images, which it returns. Live
    /// placements keep their entries in `refs` until they are released.
    pub(crate) fn reset(&mut self) -> Vec<StoredImage> {
        self.phase = Phase::Ignoring;
        self.load = None;
        self.store.clear()
    }
}

#[cfg(test)]
#[path = "kitty_tests.rs"]
pub(crate) mod kitty_tests;
