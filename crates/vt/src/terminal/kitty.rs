//! The kitty graphics protocol on the terminal: storing, placing and deleting images, and the
//! replies a program waits for.
//!
//! The wire format and the decoders live in `crate::graphics::kitty`; a placement goes through
//! the same `Handler::place` as a Sixel image, with a layout computed here.

use std::fmt::Write as _;
use std::sync::Arc;

use crate::graphics::image_store::StoredImage;
use crate::graphics::kitty::{Command, Image, Load, PlacementRef, Refusal, source_rect};
use crate::graphics::placement::{Extent, effective_cell};
use crate::graphics::{GraphicData, MAX_DIMENSION};
use crate::grid::{CursorOrigin, ScreenKind};
use crate::intern::GraphicId;

use super::State;
use super::dispatch::Handler;
use super::graphics::{Layout, Picture};

/// Most rows `r` can ask for before the screen clamps it further; it keeps the arithmetic
/// inside `u32`.
const MAX_ROWS: u32 = 4096;

const NOT_FOUND: Refusal = Refusal("ENOENT:no image with this id or number");

/// Where and how large a placement is drawn, before the screen clips it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Geometry {
    pixel_offset: (u16, u16),
    display_size: (u32, u32),
    cols: u32,
    rows: u32,
}

/// The geometry of `source` pixels placed by `command` on cells of `cell` pixels (no zero
/// axis): `X`/`Y` clamped into the cell, `c`/`r` stretching the picture over whole cells, a
/// missing one following the aspect ratio, and neither meaning one image pixel per pixel.
fn geometry(command: &Command, source: (u32, u32), cell: (u16, u16)) -> Geometry {
    let (cell_width, cell_height) = (u32::from(cell.0), u32::from(cell.1));
    let offset_x = command.cell_x.min(cell_width - 1);
    let offset_y = command.cell_y.min(cell_height - 1);
    let cols = command.cols.min(u32::from(u16::MAX));
    let rows = command.rows.min(MAX_ROWS);
    // The pixels `cells` cells leave after the offset, at least one. No overflow: both
    // factors are below 2^16.
    let span = |cells: u32, cell: u32, offset: u32| (cells * cell).saturating_sub(offset).max(1);
    // `length * numerator / denominator`, rounded, at least one.
    let scaled = |length: u32, numerator: u32, denominator: u32| {
        let exact = u64::from(length) * u64::from(numerator) + u64::from(denominator) / 2;
        u32::try_from(exact / u64::from(denominator.max(1)))
            .unwrap_or(u32::MAX)
            .max(1)
    };
    let display_size = match (cols, rows) {
        (0, 0) => source,
        (cols, 0) => {
            let width = span(cols, cell_width, offset_x);
            (width, scaled(width, source.1, source.0))
        }
        (0, rows) => {
            let height = span(rows, cell_height, offset_y);
            (scaled(height, source.0, source.1), height)
        }
        (cols, rows) => (
            span(cols, cell_width, offset_x),
            span(rows, cell_height, offset_y),
        ),
    };
    let cells = |pixels: u32, offset: u32, cell: u32| pixels.saturating_add(offset).div_ceil(cell);
    Geometry {
        // Both are below the cell size, which fits `u16`.
        pixel_offset: (offset_x as u16, offset_y as u16),
        display_size,
        cols: match cols {
            0 => cells(display_size.0, offset_x, cell_width),
            cols => cols,
        },
        rows: match rows {
            0 => cells(display_size.1, offset_y, cell_height),
            rows => rows,
        },
    }
}

/// Whether `extent` covers `row` and `col`; `None` leaves that axis free.
fn covers(extent: &Extent, row: Option<u64>, col: Option<u32>) -> bool {
    row.is_none_or(|row| (extent.top..extent.bottom).contains(&row))
        && col.is_none_or(|col| (extent.left..extent.right).contains(&col))
}

impl Handler<'_> {
    /// Runs one whole graphics command and answers it.
    pub(super) fn kitty_command(&mut self, load: Load) {
        let command = load.command;
        let result = match command.action {
            b't' | b'T' | b'q' => self.kitty_transmit(load),
            b'p' => self.kitty_put(&command),
            b'd' => {
                self.kitty_delete(&command);
                return;
            }
            _ => Err(Refusal("EINVAL:unsupported action")),
        };
        self.kitty_reply(&command, result);
    }

    /// `a=t`, `a=T`, `a=q`: decodes the payload; stores it under its id unless it is a query
    /// or has no name; places it for `T`. Returns the image id, 0 for an unnamed image.
    fn kitty_transmit(&mut self, load: Load) -> Result<u32, Refusal> {
        let command = load.command;
        if command.medium != b'd' {
            return Err(Refusal(
                "EINVAL:only direct transmission (t=d) is supported",
            ));
        }
        if command.image_id != 0 && command.image_number != 0 {
            return Err(Refusal("EINVAL:an image takes an id or a number, not both"));
        }
        let image = load.decode()?;
        if command.action == b'q' {
            return Ok(command.image_id);
        }
        let place = command.action == b'T';
        let screen = self.state.grid.screen().kind();
        let id = match (command.image_id, command.image_number) {
            // Nothing can name it again: shown once, or decoded for nothing.
            (0, 0) if place => return self.kitty_show(&command, 0, Some(image)).map(|()| 0),
            (0, 0) => return Ok(0),
            (0, _) => self.state.graphics.kitty.store.generate_id(screen),
            (id, _) => id,
        };
        // A new image under an old id ends everything that showed the old one.
        self.kitty_release_image(screen, id);
        let graphics = &mut self.state.graphics;
        let data = Arc::new(GraphicData {
            id: graphics.next_graphic_id(),
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        });
        let pushed_out = graphics.kitty.store.insert(StoredImage {
            screen,
            id,
            number: command.image_number,
            data,
            delivered: false,
        });
        graphics.retire(pushed_out);
        if place {
            self.kitty_show(&command, id, None)?;
        }
        Ok(id)
    }

    /// `a=p`: places a stored image.
    fn kitty_put(&mut self, command: &Command) -> Result<u32, Refusal> {
        let screen = self.state.grid.screen().kind();
        let store = &self.state.graphics.kitty.store;
        let id = match (command.image_id, command.image_number) {
            (0, number) => store
                .newest_numbered(screen, number)
                .map(|stored| stored.id),
            (id, _) => Some(id),
        }
        .ok_or(NOT_FOUND)?;
        self.kitty_show(command, id, None)?;
        Ok(id)
    }

    /// Places a picture at the cursor, then moves the cursor as the protocol says: `own`
    /// pixels that nothing else will show, or else the stored image `image_id` of the active
    /// screen, whose pixels every placement of it shares.
    fn kitty_show(
        &mut self,
        command: &Command,
        image_id: u32,
        own: Option<Image>,
    ) -> Result<(), Refusal> {
        if command.placeholder {
            return Err(Refusal("EINVAL:Unicode placeholders are not supported"));
        }
        if command.relative {
            return Err(Refusal("EINVAL:relative placements are not supported"));
        }
        let cell = effective_cell(self.state.cell_pixels);
        let (screen, cursor_col, screen_cols, screen_rows) = {
            let screen = self.state.grid.screen();
            let cursor_col = screen.cursor().pos.col;
            (screen.kind(), cursor_col, screen.cols(), screen.rows())
        };
        let size = match &own {
            Some(image) => (image.width, image.height),
            None => {
                let store = &self.state.graphics.kitty.store;
                let stored = store.get(screen, image_id).ok_or(NOT_FOUND)?;
                (stored.data.width, stored.data.height)
            }
        };
        let source = source_rect(command, size)?;
        let geometry = geometry(command, source.size, cell);
        // The cursor is on the screen, so there is room for one column at least.
        let room = screen_cols.saturating_sub(cursor_col).max(1);
        let cols = u16::try_from(geometry.cols)
            .unwrap_or(u16::MAX)
            .clamp(1, room);
        // An unscaled image is at most `MAX_DIMENSION` pixels high; `r` may ask for more only
        // up to the screen, which bounds the line feeds one short command can cause.
        let tallest = MAX_DIMENSION
            .div_ceil(u32::from(cell.1))
            .max(u32::from(screen_rows));
        // Fits: `tallest <= max(4096, u16::MAX)`.
        let rows = geometry.rows.clamp(1, tallest) as u16;

        if image_id != 0 && command.placement_id != 0 {
            // The same placement id again moves the placement: the old one goes.
            let State { grid, graphics, .. } = &mut *self.state;
            let old = graphics.kitty.refs.iter().find(|entry| {
                entry.screen == screen
                    && entry.image == image_id
                    && entry.placement == command.placement_id
            });
            if let Some(old) = old.map(|entry| entry.graphic) {
                graphics.release_id(old, grid);
            }
        }

        let picture = match own {
            Some(image) => Picture::Own {
                size,
                rgba: image.rgba,
            },
            None => {
                let store = &mut self.state.graphics.kitty.store;
                let stored = store.get_mut(screen, image_id).ok_or(NOT_FOUND)?;
                Picture::Shared {
                    data: Arc::clone(&stored.data),
                    // The first placement hands the pixels to the embedder.
                    deliver: !std::mem::replace(&mut stored.delivered, true),
                }
            }
        };
        let layout = Layout {
            cols,
            rows,
            walk: if command.cursor_stays { 0 } else { rows - 1 },
            source_origin: source.origin,
            source_size: source.size,
            pixel_offset: geometry.pixel_offset,
            display_size: geometry.display_size,
            z_index: command.z_index,
        };
        let graphic = self.place(picture, layout);
        self.state.graphics.kitty.refs.push(PlacementRef {
            graphic,
            screen,
            image: image_id,
            placement: command.placement_id,
            z_index: command.z_index,
        });
        if command.cursor_stays {
            return Ok(());
        }
        // The walk left the cursor on the last row of the picture; it ends right of it.
        let column = cursor_col.saturating_add(cols);
        if column >= screen_cols {
            self.state.grid.screen_mut().carriage_return();
            self.line_feed();
        } else {
            self.state
                .grid
                .screen_mut()
                .goto_column(column, CursorOrigin::Screen);
        }
        Ok(())
    }

    /// Releases every placement of the stored image `id` of `screen`.
    fn kitty_release_image(&mut self, screen: ScreenKind, id: u32) {
        let State { grid, graphics, .. } = &mut *self.state;
        let doomed: Vec<GraphicId> = graphics
            .kitty
            .refs
            .iter()
            .filter(|entry| entry.screen == screen && entry.image == id)
            .map(|entry| entry.graphic)
            .collect();
        for graphic in doomed {
            graphics.release_id(graphic, grid);
        }
    }

    /// `a=d`: releases the kitty placements of the active screen that `d` names; an upper-case
    /// `d` also forgets the stored images that are left without a placement.
    fn kitty_delete(&mut self, command: &Command) {
        let State { grid, graphics, .. } = &mut *self.state;
        let what = command.delete.to_ascii_lowercase();
        let (kind, doomed, named) = {
            let screen = grid.screen();
            let kind = screen.kind();
            let cursor = screen.cursor().pos;
            let top = screen.row_of_index(0).0;
            // Cells are 1-based on the wire; 0 and a row below the screen name nothing.
            let column = command.x.checked_sub(1);
            let row = u16::try_from(command.y)
                .ok()
                .filter(|y| (1..=screen.rows()).contains(y))
                .map(|y| screen.row_of_index(y - 1).0);
            let store = &graphics.kitty.store;
            // The stored images the command names outright.
            let named: Vec<u32> = match what {
                b'i' => Some(command.image_id)
                    .filter(|id| *id != 0)
                    .into_iter()
                    .collect(),
                b'n' => store
                    .newest_numbered(kind, command.image_number)
                    .map(|stored| stored.id)
                    .into_iter()
                    .collect(),
                b'r' => store
                    .ids(kind)
                    .filter(|id| (command.x..=command.y).contains(id))
                    .collect(),
                _ => Vec::new(),
            };
            let on_screen = graphics
                .kitty
                .refs
                .iter()
                .filter(|entry| entry.screen == kind);
            let doomed: Vec<PlacementRef> = on_screen
                .filter(|entry| {
                    let extent = graphics.extent_of(entry.graphic, grid);
                    let over = |row: Option<u64>, col: Option<u32>| {
                        extent
                            .as_ref()
                            .is_some_and(|extent| covers(extent, row, col))
                    };
                    let same_placement =
                        command.placement_id == 0 || entry.placement == command.placement_id;
                    match what {
                        b'a' => extent.as_ref().is_some_and(|extent| extent.bottom > top),
                        b'i' | b'n' => named.contains(&entry.image) && same_placement,
                        b'r' => named.contains(&entry.image),
                        b'c' => over(Some(cursor.row.0), Some(u32::from(cursor.col))),
                        b'p' => column.is_some() && row.is_some() && over(row, column),
                        b'q' => {
                            column.is_some()
                                && row.is_some()
                                && over(row, column)
                                && entry.z_index == command.z_index
                        }
                        b'x' => column.is_some() && over(None, column),
                        b'y' => row.is_some() && over(row, None),
                        b'z' => entry.z_index == command.z_index,
                        _ => false,
                    }
                })
                .copied()
                .collect();
            (kind, doomed, named)
        };
        for entry in &doomed {
            graphics.release_id(entry.graphic, grid);
        }
        if !command.delete.is_ascii_uppercase() {
            return;
        }
        let orphans = doomed.iter().map(|entry| entry.image).chain(named);
        for id in orphans.filter(|id| *id != 0) {
            let placed = |entry: &PlacementRef| entry.screen == kind && entry.image == id;
            if !graphics.kitty.refs.iter().any(placed) {
                let forgotten = graphics.kitty.store.remove(kind, id);
                graphics.retire(forgotten);
            }
        }
    }

    /// Answers a command: `OK` or the refusal, named by the ids the program can match. Silent
    /// when the command named no image, under `q=2`, and for `OK` under `q=1`.
    fn kitty_reply(&mut self, command: &Command, result: Result<u32, Refusal>) {
        let (id, message) = match result {
            Ok(id) => (id, "OK"),
            Err(refusal) => {
                self.unhandled();
                (command.image_id, refusal.0)
            }
        };
        let silent = command.quiet >= 2 || (command.quiet == 1 && result.is_ok());
        if silent || (id == 0 && command.image_number == 0) {
            return;
        }
        let scratch = &mut self.state.scratch;
        scratch.clear();
        scratch.push_str("\x1b_G");
        let mut separator = "";
        let keys = [
            ('i', id),
            ('I', command.image_number),
            ('p', command.placement_id),
        ];
        for (key, value) in keys.into_iter().filter(|(_, value)| *value != 0) {
            // Writing to a `String` cannot fail.
            let _ = write!(scratch, "{separator}{key}={value}");
            separator = ",";
        }
        let _ = write!(scratch, ";{message}\x1b\\");
        self.out.push_reply(scratch.as_bytes());
    }
}

#[cfg(test)]
#[path = "kitty_tests.rs"]
mod kitty_tests;
