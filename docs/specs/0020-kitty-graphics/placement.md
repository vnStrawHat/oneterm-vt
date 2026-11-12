# Store, placing, deleting

## Image store

`graphics/image_store.rs`: `ImageStore { images: Vec<StoredImage>, bytes }`, oldest first. A
`StoredImage` is `{ screen, id, number, data: Arc<GraphicData>, delivered }`; `(screen, id)` is
its key, `screen` being the one that was active when it arrived, and `data.id` is a fresh
`GraphicId` from the terminal's counter.

- `a=t` and `a=T` with `i`: every placement of an image with the same key is released, then
  the image is replaced.
- With `I` and no `i`: the engine picks a free id from `0x8000_0000` upwards and answers with
  it. `a=p` with `I` names the newest image of that number.
- Without both: `a=t` decodes and forgets; `a=T` places the pixels as their own placement.
- Past 128 MiB or 1,024 images the oldest image is forgotten. RIS empties the store.
- `insert`, `remove` and `clear` return what the store gave up; `GraphicsState::retire` queues
  `GraphicReleased(data.id)` for each one that was delivered and that no placement shows.

## Placing (`a=T`, `a=p`)

1. Refuse `U=1`, `P`, `Q`. Find the image (`ENOENT`).
2. Source rectangle: `x`, `y` must lie inside the image; `w = 0` and `h = 0` mean "to the
   edge"; both are clipped to the edge. Nothing is copied: the rectangle goes into the
   placement.
3. Cell: `effective_cell(cell_pixels)`. `X` and `Y` are clamped below the cell size.
4. Display size and footprint, with `(sw, sh)` the source size and `(cw, ch)` the cell:

   | `c`, `r` | `display_size` | cells |
   |---|---|---|
   | neither | `(sw, sh)` | `ceil((sw + X) / cw)`, `ceil((sh + Y) / ch)` |
   | both | `(c * cw - X, r * ch - Y)` | `c`, `r` |
   | `c` only | width as above, height `width * sh / sw` rounded | `c`, `ceil((height + Y) / ch)` |
   | `r` only | the mirror image | `ceil((width + X) / cw)`, `r` |

   Columns are clipped at the right edge of the screen. Rows are at least 1 and at most
   `max(screen rows, ceil(4096 / ch))`: an unscaled image never needs more, and the bound
   limits the line feeds one short command can cause.
5. A placement id `p` that this image already has on this screen is released first.
6. `Handler::place(picture, layout)` does the rest as for Sixel: a `GraphicId` for the
   placement, extras, anchor, `admit`, stamped rows, `release_covered`. `Picture::Own` (Sixel,
   an unnamed kitty image) queues its pixels under the placement's id; `Picture::Shared`
   queues the stored `Arc` only when `delivered` was false, and sets it.
7. Cursor: with `C=0` the walk is `rows - 1` line feeds (through the dispatcher's line feed, so
   regions scroll and the image moves with its rows), then the column advances by `cols`; at or
   past the last column it becomes a carriage return and one more line feed. With `C=1` the
   walk is 0 and the image continues below the cursor row.

`graphics.kitty.refs` records `{ graphic, screen, image, placement, z_index }` for every kitty
placement; `GraphicsState::release` drops the entry, whatever the release cause.

## Budgets with shared images

- `live_bytes` counts an image once, however many placements show it; `admit` and `release`
  ask "does another placement show this image".
- `release` queues `GraphicReleased(image)` after the last placement of an image the store does
  not hold.
- A pending image dropped at the 128 MiB queue ceiling releases all its placements and, if it
  is stored, becomes undelivered: its next placement queues it again.

## New `Placement` fields

| Field | Meaning | Sixel |
|---|---|---|
| `image: GraphicId` | the `GraphicData` to draw | `== id` |
| `source_origin`, `source_size` | the part of the image shown, in image pixels | the whole image |
| `pixel_offset: (u16, u16)` | where the picture starts inside its top-left cell | `(0, 0)` |
| `display_size: (u32, u32)` | the size to draw that part at, in device pixels of the cell size at placement time | `pixel_size` |
| `z_index: i32` | layer; below 0 goes under the text | `0` |

`SnapshotPlacement` mirrors them. `release_covered` compares layers: an older placement inside
the newest one's extent is released only when its `z_index <=` the newest one's.

## Deleting (`a=d`)

Only placements in `kitty.refs` of the active screen are candidates. A lower-case `d` releases
placements; the upper-case letter also forgets every image that lost its last placement.

| `d` | Releases |
|---|---|
| `a`, `A` | every placement with a row on the screen (not one wholly in history) |
| `i`, `I` | placements of image `i`, only `p` when given |
| `n`, `N` | the same for the newest image with number `I` |
| `c`, `C` | placements over the cursor cell |
| `p`, `P` | placements over cell `x`, `y` (1-based) |
| `q`, `Q` | the same, and layer `z` |
| `x`, `X` / `y`, `Y` | placements over column `x` / row `y` |
| `z`, `Z` | placements on layer `z` |
| `r`, `R` | placements of the images with `x <= id <= y` |

`I`, `N` and `R` forget the named images even when they had no placement. Anything else
(`f`, `F`, an unknown letter) is ignored. Released cells keep naming the dead `GraphicId`,
which resolves to nothing, as after every other release.
